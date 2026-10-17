use crate::mounts::{Mount, containing};
use anyhow::Result;
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fs::File,
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

pub(super) const MASK: u64 = libc::FAN_MODIFY
    | libc::FAN_ATTRIB
    | libc::FAN_CLOSE_WRITE
    | libc::FAN_CREATE
    | libc::FAN_DELETE
    | libc::FAN_MOVED_FROM
    | libc::FAN_MOVED_TO
    | libc::FAN_ONDIR;

/// How event file handles of one marked filesystem are decoded.
pub(super) enum Source {
    /// Ordinary filesystem: decode through a current mountpoint, opened per
    /// event so that Disko never keeps a mount busy (`umount` keeps working).
    Mounted { points: Vec<PathBuf> },
    /// Btrfs: decode through a private detached top-level mount and translate
    /// subvolume paths to where each subvolume is mounted.
    Detached {
        root: File,
        map: Vec<(PathBuf, PathBuf)>,
    },
}

pub(super) struct Marked {
    pub(super) fsid: [u8; 8],
    pub(super) source: Source,
}

/// Marked filesystems by mount group, refreshed whenever the mount table changes.
#[derive(Default)]
pub(super) struct Sources {
    pub(super) marked: HashMap<String, Marked>,
}

impl Sources {
    /// Mark filesystems that newly appeared below the roots, update decoding
    /// for moved or removed mounts, and return the device IDs now covered.
    pub(super) fn refresh(
        &mut self,
        fd: i32,
        mounts: &[Mount],
        roots: &[PathBuf],
        excludes: &[PathBuf],
    ) -> HashSet<u64> {
        let wanted = |p: &Path| {
            !excludes.iter().any(|e| p.starts_with(e))
                && !p.components().any(|c| c.as_os_str() == ".snapshots")
        };
        // Mounts below a root, plus the mount holding each root itself.
        let mut candidates: Vec<(&Mount, PathBuf)> = mounts
            .iter()
            .filter(|m| roots.iter().any(|r| m.point.starts_with(r)) && wanted(&m.point))
            .map(|m| (m, m.point.clone()))
            .collect();
        for root in roots {
            if let Some(m) = containing(mounts, root) {
                candidates.push((m, root.clone()));
            }
        }
        for (m, path) in candidates {
            let group = m.group();
            if self.marked.contains_key(&group) || m.fstype == "overlay" {
                continue;
            }
            match mark(fd, &path, m) {
                Ok(Some(marked)) => {
                    self.marked.insert(group, marked);
                }
                Ok(None) => {}
                Err(e) => eprintln!(
                    "fanotify: {}: {e}; using directory inotify watches there",
                    path.display()
                ),
            }
        }
        let mut devices = HashSet::new();
        self.marked.retain(|group, marked| {
            let members: Vec<_> = mounts.iter().filter(|m| &m.group() == group).collect();
            if members.is_empty() {
                return false; // Unmounted: the kernel drops the mark with the superblock.
            }
            for m in &members {
                if let Ok(md) = std::fs::metadata(&m.point) {
                    devices.insert(md.dev());
                }
            }
            match &mut marked.source {
                Source::Mounted { points } => {
                    *points = members.iter().map(|m| m.point.clone()).collect();
                    points.sort_by_key(|p| p.as_os_str().len());
                }
                Source::Detached { map, .. } => {
                    *map = members
                        .iter()
                        .map(|m| (m.root.clone(), m.point.clone()))
                        .collect();
                }
            }
            true
        });
        devices
    }

    pub(super) fn is_empty(&self) -> bool {
        self.marked.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.marked.len()
    }
}

/// Place a filesystem-wide mark. `Ok(None)` means the filesystem cannot be
/// marked (unsupported or unprivileged) and inotify remains the fallback.
fn mark(fd: i32, path: &Path, m: &Mount) -> Result<Option<Marked>> {
    let Ok(file) = File::open(path) else {
        return Ok(None);
    };
    if !file.metadata().is_ok_and(|md| md.is_dir()) {
        return Ok(None);
    }
    let name = CString::new(path.as_os_str().as_bytes())?;
    if unsafe {
        libc::fanotify_mark(
            fd,
            libc::FAN_MARK_ADD | libc::FAN_MARK_FILESYSTEM,
            MASK,
            libc::AT_FDCWD,
            name.as_ptr(),
        )
    } == 0
    {
        let Some(fsid) = super::btrfs::fsid(&file) else {
            return Ok(None);
        };
        return Ok(Some(Marked {
            fsid,
            source: Source::Mounted {
                points: vec![m.point.clone()],
            },
        }));
    }
    // Btrfs refuses filesystem marks on subvolumes other than the top level.
    if io::Error::last_os_error().raw_os_error() != Some(libc::EXDEV) || m.fstype != "btrfs" {
        return Ok(None);
    }
    let root = super::btrfs::whole_btrfs(fd, &m.source)?;
    let Some(fsid) = super::btrfs::fsid(&root) else {
        return Ok(None);
    };
    Ok(Some(Marked {
        fsid,
        source: Source::Detached {
            root,
            map: Vec::new(),
        },
    }))
}
