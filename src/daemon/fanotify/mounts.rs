use anyhow::Result;
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fs::File,
    mem::MaybeUninit,
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Path, PathBuf},
};

pub(super) type Handles = HashMap<[u8; 8], Vec<File>>;
pub(super) fn mark(
    fd: i32,
    roots: &[PathBuf],
    excludes: &[PathBuf],
) -> Result<(Handles, HashSet<u64>)> {
    let mut paths = roots.to_vec();
    for line in std::fs::read_to_string("/proc/self/mountinfo")?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 6 {
            continue;
        }
        let path = PathBuf::from(fields[4].replace("\\040", " ").replace("\\134", "\\"));
        if roots.iter().any(|r| path.starts_with(r))
            && !excludes.iter().any(|p| path.starts_with(p))
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    let mut handles = HashMap::<[u8; 8], Vec<File>>::new();
    let mut devices = HashSet::new();
    let mask = libc::FAN_MODIFY
        | libc::FAN_ATTRIB
        | libc::FAN_CLOSE_WRITE
        | libc::FAN_CREATE
        | libc::FAN_DELETE
        | libc::FAN_MOVED_FROM
        | libc::FAN_MOVED_TO
        | libc::FAN_ONDIR;
    for path in paths {
        if path.components().any(|c| c.as_os_str() == ".snapshots") {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let name = CString::new(path.as_os_str().as_bytes())?;
        if unsafe {
            libc::fanotify_mark(
                fd,
                libc::FAN_MARK_ADD | libc::FAN_MARK_FILESYSTEM,
                mask,
                libc::AT_FDCWD,
                name.as_ptr(),
            )
        } != 0
        {
            continue;
        }
        let mut stat = MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            continue;
        }
        let stat = unsafe { stat.assume_init() };
        let fsid: [u8; 8] = unsafe {
            std::slice::from_raw_parts((&stat.f_fsid as *const libc::fsid_t).cast::<u8>(), 8)
        }
        .try_into()
        .unwrap();
        handles.entry(fsid).or_default().push(file);
        devices.insert(metadata.dev());
    }
    Ok((handles, devices))
}

pub(super) fn resolve(handles: &Handles, fsid: [u8; 8], handle: &[u8]) -> Option<PathBuf> {
    use std::os::fd::{FromRawFd, OwnedFd};
    // Copy into aligned storage before passing the variable-size file_handle to libc.
    let mut aligned = vec![0_u64; handle.len().div_ceil(8)];
    unsafe {
        std::ptr::copy_nonoverlapping(
            handle.as_ptr(),
            aligned.as_mut_ptr().cast::<u8>(),
            handle.len(),
        );
    }
    for mount in handles.get(&fsid)? {
        let raw = unsafe {
            libc::open_by_handle_at(
                mount.as_raw_fd(),
                aligned.as_mut_ptr().cast(),
                libc::O_PATH | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            continue;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let Ok(path) = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())) else {
            continue;
        };
        if path.is_absolute()
            && !path.as_os_str().as_bytes().ends_with(b" (deleted)")
            && path != Path::new("/")
        {
            return Some(path);
        }
        if path == Path::new("/") {
            return Some(path);
        }
    }
    None
}
