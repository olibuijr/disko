use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Enumeration priority: Front jobs run first, Tail jobs mirror the old
/// root-last placement of large late roots such as /mnt.
#[derive(Clone, Copy)]
pub(super) enum Place {
    Front,
    Regular,
    Tail,
}

impl Place {
    /// Stored form in `scan_queue`.
    pub(super) fn code(self) -> i64 {
        self as i64
    }
    pub(super) fn from_code(code: i64) -> Self {
        match code {
            0 => Place::Front,
            2 => Place::Tail,
            _ => Place::Regular,
        }
    }
}

pub(super) struct Filter {
    excludes: Vec<PathBuf>,
}

impl Filter {
    pub(super) fn new(excludes: Vec<PathBuf>) -> Self {
        Self { excludes }
    }
    pub(super) fn excluded(&self, p: &Path) -> bool {
        self.excludes.iter().any(|e| p.starts_with(e))
            || p.components()
                .any(|x| x.as_os_str() == OsStr::new(".snapshots"))
    }
}

pub(super) struct Job {
    pub(super) path: PathBuf,
    pub(super) id: i64,
    pub(super) seen: i64,
    pub(super) place: Place,
}

/// One enumerated directory. The writer replays children and failures; sibling
/// `seen` generations stay per-directory, exactly as in the serial walk.
pub(super) struct DirResult {
    pub(super) path: PathBuf,
    pub(super) id: i64,
    pub(super) seen: i64,
    pub(super) place: Place,
    pub(super) complete: bool,
    pub(super) children: Vec<(OsString, fs::Metadata)>,
    pub(super) read_failed: Option<std::io::Error>,
    pub(super) entry_failed: Vec<std::io::Error>,
    pub(super) stat_failed: Vec<(OsString, std::io::Error)>,
}

pub(super) fn crew_size(requested: u32) -> usize {
    match requested {
        0 => thread::available_parallelism().map_or(2, |n| n.get()),
        n => n as usize,
    }
    .clamp(1, 16)
    .min(if requested == 0 { 4 } else { 16 })
}

pub(super) fn walk_dir(stop: &AtomicBool, filter: &Filter, job: &Job) -> DirResult {
    let mut r = DirResult {
        path: job.path.clone(),
        id: job.id,
        seen: job.seen,
        place: job.place,
        complete: true,
        children: Vec::new(),
        read_failed: None,
        entry_failed: Vec::new(),
        stat_failed: Vec::new(),
    };
    if stop.load(Ordering::Relaxed) {
        r.complete = false;
        return r;
    }
    let rd = match fs::read_dir(&job.path) {
        Ok(rd) => rd,
        Err(e) => {
            r.read_failed = Some(e);
            r.complete = false;
            return r;
        }
    };
    for entry in rd {
        if stop.load(Ordering::Relaxed) {
            r.complete = false;
            return r;
        }
        match entry {
            Ok(entry) => {
                let child = entry.path();
                if filter.excluded(&child) {
                    continue;
                }
                match fs::symlink_metadata(&child) {
                    Ok(m) => r.children.push((entry.file_name(), m)),
                    Err(e) => r.stat_failed.push((entry.file_name(), e)),
                }
            }
            Err(e) => r.entry_failed.push(e),
        }
    }
    r
}

/// Keep the former root-first priority: home and var before the rest, /mnt last.
pub(super) fn place_for(inherited: Place, root: bool, name: &OsStr) -> Place {
    if matches!(inherited, Place::Tail) {
        return Place::Tail;
    }
    if !root {
        return Place::Regular;
    }
    match name.as_bytes() {
        b"home" | b"var" | b"usr" | b"tmp" => Place::Front,
        b"mnt" => Place::Tail,
        _ => Place::Regular,
    }
}
