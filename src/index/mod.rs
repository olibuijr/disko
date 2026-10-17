use anyhow::{Context, Result, bail};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod commit;
mod db;
mod paths;
mod pool;
mod scan;
mod updates;
mod walk;
pub use db::{now, open, set_meta};
pub use paths::{absolute, path_of, resolve};

#[derive(Clone, Copy, Default, Debug)]
struct Delta {
    bytes: i64,
    blocks: i64,
    files: i64,
    entries: i64,
}
impl Delta {
    fn add(&mut self, d: Self) {
        self.bytes += d.bytes;
        self.blocks += d.blocks;
        self.files += d.files;
        self.entries += d.entries;
    }
}

pub struct Index {
    pub c: Connection,
    parents: HashMap<i64, i64>,
    deltas: HashMap<i64, Delta>,
    dirty: usize,
    pub visited: u64,
    pub errors: u64,
    pub watch_errors: u64,
    /// Ordered so a directory's watched descendants form one contiguous range.
    pub watched: BTreeSet<PathBuf>,
    /// Mountpoints recorded but not descended (overlays, relocated binds).
    pub skip: HashSet<PathBuf>,
    pub excludes: Vec<PathBuf>,
    pub roots: Vec<PathBuf>,
    filter: std::sync::Arc<walk::Filter>,
    jobs: u32,
    pub watcher: Option<RecommendedWatcher>,
    pub filesystem_devices: HashSet<u64>,
    last_commit: Instant,
    generation: i64,
    current_path: PathBuf,
    last_sample: i64,
    pub stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Index {
    pub fn new(
        c: Connection,
        roots: Vec<PathBuf>,
        excludes: Vec<PathBuf>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        jobs: u32,
    ) -> Result<Self> {
        c.execute_batch("BEGIN IMMEDIATE")?;
        let filter = std::sync::Arc::new(walk::Filter::new(excludes.clone()));
        Ok(Self {
            c,
            parents: HashMap::new(),
            deltas: HashMap::new(),
            dirty: 0,
            visited: 0,
            errors: 0,
            watch_errors: 0,
            watched: BTreeSet::new(),
            skip: HashSet::new(),
            excludes,
            roots,
            filter,
            jobs,
            watcher: None,
            filesystem_devices: HashSet::new(),
            last_commit: Instant::now(),
            generation: SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as i64,
            current_path: PathBuf::from("/"),
            last_sample: 0,
            stop,
        })
    }

    pub fn excluded(&self, p: &Path) -> bool {
        self.filter.excluded(p)
    }

    pub fn relevant(&self, p: &Path) -> bool {
        !self.excluded(p)
            && self.roots.iter().any(|r| p.starts_with(r))
            && !self.skip.iter().any(|s| p != s && p.starts_with(s))
    }

    /// Paths in `watched` at or below `dir`.
    pub(super) fn watched_under(&self, dir: &Path) -> Vec<PathBuf> {
        self.watched
            .range(dir.to_owned()..)
            .take_while(|p| p.starts_with(dir))
            .cloned()
            .collect()
    }

    /// Forget watches below a mountpoint that was mounted or unmounted; the
    /// kernel already dropped them, so a later sweep must be free to re-add.
    pub fn forget_watches(&mut self, dir: &Path) {
        for p in self.watched_under(dir) {
            if let Some(w) = &mut self.watcher {
                let _ = w.unwatch(&p);
            }
            self.watched.remove(&p);
        }
    }

    /// Replace the skipped-mount set, emptying newly skipped mountpoints.
    pub fn set_skip(&mut self, skip: HashSet<PathBuf>) -> Result<()> {
        let added: Vec<_> = skip.difference(&self.skip).cloned().collect();
        self.skip = skip;
        for point in added {
            if let Some(id) = resolve(&self.c, &point)? {
                self.prune_children(id)?;
            }
            self.forget_watches(&point);
        }
        Ok(())
    }

    pub(super) fn error(&mut self, p: &Path, e: &dyn std::fmt::Display) {
        self.errors += 1;
        if self.errors <= 10 || self.errors.is_multiple_of(1000) {
            eprintln!("metadata: {}: {e}", p.display());
        }
    }

    /// Install a directory watch unless a filesystem-wide fanotify mark already
    /// covers it. `m` is the directory's own metadata when the caller has it.
    pub(super) fn watch_dir(&mut self, p: &Path, m: Option<&fs::Metadata>) {
        if !self.filesystem_devices.is_empty() {
            let dev = match m {
                Some(m) => Some(m.dev()),
                None => fs::metadata(p).ok().map(|m| m.dev()),
            };
            if let Some(dev) = dev {
                if self.filesystem_devices.contains(&dev) {
                    return;
                }
                if nested_subvolume(p, &self.filesystem_devices) {
                    self.filesystem_devices.insert(dev);
                    return;
                }
            }
        }
        if self.watched.contains(p) {
            return;
        }
        if let Some(w) = &mut self.watcher {
            match w.watch(p, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    self.watched.insert(p.to_owned());
                }
                Err(e) => {
                    self.watch_errors += 1;
                    if self.watch_errors <= 5 {
                        eprintln!(
                            "watch: {}: {e}; periodic reconciliation remains active",
                            p.display()
                        );
                    }
                }
            }
        }
    }
}
/// A btrfs subvolume nested in a covered one shares its superblock, and so its
/// fanotify mark, but reports its own st_dev. Subvolume roots are inode 256;
/// a mount root with that inode is a different filesystem and is not covered.
fn nested_subvolume(p: &Path, covered: &HashSet<u64>) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Some(parent) = p.parent() else {
        return false;
    };
    if !fs::symlink_metadata(parent).is_ok_and(|m| covered.contains(&m.dev())) {
        return false;
    }
    let Ok(name) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stx = std::mem::MaybeUninit::<libc::statx>::uninit();
    if unsafe {
        libc::statx(
            libc::AT_FDCWD,
            name.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
            libc::STATX_INO,
            stx.as_mut_ptr(),
        )
    } != 0
    {
        return false;
    }
    let stx = unsafe { stx.assume_init() };
    let mount_root = libc::STATX_ATTR_MOUNT_ROOT as u64;
    stx.stx_ino == 256
        && stx.stx_attributes_mask & mount_root != 0
        && stx.stx_attributes & mount_root == 0
}

impl Drop for Index {
    fn drop(&mut self) {
        let _ = self.commit();
        let _ = self
            .c
            .execute_batch("COMMIT; PRAGMA wal_checkpoint(PASSIVE)");
    }
}
