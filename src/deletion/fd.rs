use anyhow::{Context, Result, bail};
use std::{
    ffi::{CString, OsStr},
    io,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path},
};

pub fn open_dir(parent: i32, name: &OsStr) -> Result<OwnedFd> {
    let name = CString::new(name.as_bytes())?;
    // O_NOFOLLOW is applied to every path component, including during recursion.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub fn open_parent(path: &Path) -> Result<OwnedFd> {
    let mut fd = open_dir(libc::AT_FDCWD, OsStr::new("/"))?;
    for part in path.parent().context("missing parent")?.components() {
        if let Component::Normal(name) = part {
            fd = open_dir(fd.as_raw_fd(), name)?;
        }
    }
    Ok(fd)
}

pub fn stat(parent: i32, name: &OsStr) -> Result<libc::stat> {
    let name = CString::new(name.as_bytes())?;
    let mut value = MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent,
            name.as_ptr(),
            value.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { value.assume_init() })
}

pub fn mount_id(fd: i32) -> Result<String> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}"))?;
    Ok(info
        .lines()
        .find_map(|s| s.strip_prefix("mnt_id:"))
        .context("missing mount ID")?
        .trim()
        .into())
}

pub fn unlink(parent: i32, name: &OsStr, directory: bool) -> Result<()> {
    let name = CString::new(name.as_bytes())?;
    let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
    if unsafe { libc::unlinkat(parent, name.as_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

pub fn same(stat: &libc::stat, dev: u64, ino: u64) -> Result<()> {
    if stat.st_dev != dev || stat.st_ino != ino {
        bail!("entry changed since preview; refresh and try again");
    }
    Ok(())
}
