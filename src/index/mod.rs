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
mod gc;
mod growth;
mod known;
mod paths;
mod pool;
mod reconcile;
mod rename;
mod scan;
mod updates;
mod walk;
mod watch;
pub use db::{now, open, set_meta};
pub use gc::GC_ROWS;
pub use paths::{absolute, is_live, path_of, resolve};

/// Unix time of the last sign of progress (a commit, or a walker working
/// through a large directory). The daemon's watchdog reads it.
pub static PROGRESS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// What the writer is doing, for the watchdog's stall report.
pub static PHASE: std::sync::Mutex<&'static str> = std::sync::Mutex::new("starting");

pub fn progress() {
    PROGRESS.store(now(), std::sync::atomic::Ordering::Relaxed);
}
pub fn phase(p: &'static str) {
    if let Ok(mut g) = PHASE.lock() {
        *g = p;
    }
}

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
    /// Set during a full scan: pending directories are kept in `scan_queue`.
    resumable: bool,
    /// Set during an incremental scan: folders whose mtime and ctime match the
    /// index are not reread; only their known subfolders are checked.
    incremental: bool,
    /// Detached rows may be waiting for `collect_garbage`.
    garbage: bool,
    /// Start of the last completed scan. Times are stored in whole seconds, so
    /// a folder changed at or after this may match its row and still differ
    /// (the "racy timestamp" problem); such folders are always reread.
    trust_before: i64,
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
            resumable: false,
            incremental: false,
            garbage: true,
            trust_before: 0,
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

    pub(super) fn error(&mut self, p: &Path, e: &dyn std::fmt::Display) {
        self.errors += 1;
        if self.errors <= 10 || self.errors.is_multiple_of(1000) {
            eprintln!("metadata: {}: {e}", p.display());
        }
    }
}
impl Drop for Index {
    fn drop(&mut self) {
        let _ = self.commit();
        let _ = self
            .c
            .execute_batch("COMMIT; PRAGMA wal_checkpoint(PASSIVE)");
    }
}
