use super::mounts::{Source, Sources};
use std::{
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
};

impl Sources {
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
