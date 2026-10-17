use crate::mounts::{Mount, containing};
use anyhow::{Result, bail};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    fs::File,
    io,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
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
enum Source {
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

struct Marked {
    fsid: [u8; 8],
    source: Source,
}

/// Marked filesystems by mount group, refreshed whenever the mount table changes.
#[derive(Default)]
pub(super) struct Sources {
    marked: HashMap<String, Marked>,
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

    pub(super) fn resolve(&self, fsid: [u8; 8], handle: &[u8]) -> Option<PathBuf> {
        // Copy into aligned storage before passing the variable-size file_handle to libc.
        let mut aligned = vec![0_u64; handle.len().div_ceil(8)];
        unsafe {
            std::ptr::copy_nonoverlapping(
                handle.as_ptr(),
                aligned.as_mut_ptr().cast::<u8>(),
                handle.len(),
            );
        }
        for marked in self.marked.values().filter(|m| m.fsid == fsid) {
            match &marked.source {
                Source::Mounted { points } => {
                    for point in points {
                        let Ok(dir) = File::open(point) else {
                            continue;
                        };
                        if let Some(path) = decode(&dir, &mut aligned) {
                            return Some(path);
                        }
                    }
                }
                Source::Detached { root, map } => {
                    if let Some(path) = decode(root, &mut aligned) {
                        return translate(map, &path);
                    }
                }
            }
        }
        None
    }
}

fn decode(dir: &File, handle: &mut [u64]) -> Option<PathBuf> {
    let raw = unsafe {
        libc::open_by_handle_at(
            dir.as_raw_fd(),
            handle.as_mut_ptr().cast(),
            libc::O_PATH | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let path = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).ok()?;
    (path.is_absolute() && !path.as_os_str().as_bytes().ends_with(b" (deleted)")).then_some(path)
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
        let Some(fsid) = fsid(&file) else {
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
    let root = whole_btrfs(fd, &m.source)?;
    let Some(fsid) = fsid(&root) else {
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

/// Mark a whole btrfs filesystem through a private, read-only, detached mount
/// of its top-level subvolume. Nothing is attached to the mount tree; the mount
/// lives exactly as long as the returned directory handle.
fn whole_btrfs(fd: i32, source: &str) -> Result<File> {
    const FSOPEN_CLOEXEC: libc::c_uint = 1;
    const FSCONFIG_SET_STRING: libc::c_uint = 1;
    const FSCONFIG_CMD_CREATE: libc::c_uint = 6;
    const FSMOUNT_CLOEXEC: libc::c_uint = 1;
    const MOUNT_ATTR_RDONLY: libc::c_uint = 1;
    fn owned(raw: libc::c_long, what: &str) -> Result<OwnedFd> {
        if raw < 0 {
            bail!("{what}: {}", io::Error::last_os_error());
        }
        Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
    }
    let context = owned(
        unsafe { libc::syscall(libc::SYS_fsopen, c"btrfs".as_ptr(), FSOPEN_CLOEXEC) },
        "fsopen",
    )?;
    let source = CString::new(source)?;
    let settings: [(&CStr, &CStr); 2] = [(c"source", &source), (c"subvolid", c"5")];
    for (key, value) in settings {
        if unsafe {
            libc::syscall(
                libc::SYS_fsconfig,
                context.as_raw_fd(),
                FSCONFIG_SET_STRING,
                key.as_ptr(),
                value.as_ptr(),
                0,
            )
        } < 0
        {
            bail!("fsconfig {key:?}: {}", io::Error::last_os_error());
        }
    }
    if unsafe {
        libc::syscall(
            libc::SYS_fsconfig,
            context.as_raw_fd(),
            FSCONFIG_CMD_CREATE,
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(),
            0,
        )
    } < 0
    {
        bail!("fsconfig create: {}", io::Error::last_os_error());
    }
    let mount = owned(
        unsafe {
            libc::syscall(
                libc::SYS_fsmount,
                context.as_raw_fd(),
                FSMOUNT_CLOEXEC,
                MOUNT_ATTR_RDONLY,
            )
        },
        "fsmount",
    )?;
    // fsmount returns an O_PATH descriptor, which fanotify_mark and
    // open_by_handle_at reject; open the mount root as a real directory.
    let root = unsafe {
        libc::openat(
            mount.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if root < 0 {
        bail!("open top-level subvolume: {}", io::Error::last_os_error());
    }
    let root = unsafe { File::from_raw_fd(root) };
    if unsafe {
        libc::fanotify_mark(
            fd,
            libc::FAN_MARK_ADD | libc::FAN_MARK_FILESYSTEM,
            MASK,
            root.as_raw_fd(),
            c".".as_ptr(),
        )
    } != 0
    {
        bail!("fanotify_mark: {}", io::Error::last_os_error());
    }
    Ok(root)
}

fn fsid(file: &File) -> Option<[u8; 8]> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    unsafe { std::slice::from_raw_parts((&stat.f_fsid as *const libc::fsid_t).cast::<u8>(), 8) }
        .try_into()
        .ok()
}

/// Map a path inside the filesystem to where it is mounted, preferring the
/// most specific subvolume.
fn translate(map: &[(PathBuf, PathBuf)], path: &Path) -> Option<PathBuf> {
    map.iter()
        .filter_map(|(root, point)| {
            let rest = path.strip_prefix(root).ok()?;
            Some((root.as_os_str().len(), point.join(rest)))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_subvolume_paths_to_mountpoints() {
        let map: Vec<_> = [("/@", "/"), ("/@home", "/home"), ("/@log", "/var/log")]
            .iter()
            .map(|(r, p)| (PathBuf::from(r), PathBuf::from(p)))
            .collect();
        let t = |p: &str| translate(&map, Path::new(p));
        assert_eq!(t("/@home/user/x").unwrap(), Path::new("/home/user/x"));
        assert_eq!(t("/@/var/tmp").unwrap(), Path::new("/var/tmp"));
        assert_eq!(t("/@log").unwrap(), Path::new("/var/log"));
        assert_eq!(t("/@/swap/file").unwrap(), Path::new("/swap/file"));
        assert_eq!(t("/@homework/x"), None); // component-wise, not string prefix
        assert_eq!(t("/@factory/etc"), None); // unmounted subvolume
    }
}
