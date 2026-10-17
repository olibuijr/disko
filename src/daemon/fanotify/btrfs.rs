use anyhow::{Result, bail};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io,
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

/// Mark a whole btrfs filesystem through a private, read-only, detached mount
/// of its top-level subvolume. Nothing is attached to the mount tree; the mount
/// lives exactly as long as the returned directory handle.
pub(super) fn whole_btrfs(fd: i32, source: &str) -> Result<File> {
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
            super::mounts::MASK,
            root.as_raw_fd(),
            c".".as_ptr(),
        )
    } != 0
    {
        bail!("fanotify_mark: {}", io::Error::last_os_error());
    }
    Ok(root)
}

pub(super) fn fsid(file: &File) -> Option<[u8; 8]> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    unsafe { std::slice::from_raw_parts((&stat.f_fsid as *const libc::fsid_t).cast::<u8>(), 8) }
        .try_into()
        .ok()
}
