mod fd;
use crate::index::absolute;
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsString,
    os::fd::{AsRawFd, OwnedFd},
    path::{Path, PathBuf},
};

pub struct Target {
    parent: OwnedFd,
    name: OsString,
    dev: u64,
    ino: u64,
    pub path: PathBuf,
}
impl Target {
    pub fn prepare(path: &Path, dev: u64, ino: u64) -> Result<Self> {
        let path = absolute(path)?;
        let allowed = ["/home", "/mnt", "/media", "/tmp", "/var/tmp"];
        if !allowed
            .iter()
            .any(|p| path.starts_with(p) && path != Path::new(p))
        {
            bail!("deletion is limited to entries below /home, /mnt, /media, /tmp and /var/tmp");
        }
        if path.components().any(|p| p.as_os_str() == ".snapshots") {
            bail!("snapshot deletion is not supported");
        }
        let parent = fd::open_parent(&path)?;
        let name = path.file_name().context("missing filename")?.to_owned();
        let stat = fd::stat(parent.as_raw_fd(), &name)?;
        fd::same(&stat, dev, ino)?;
        if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
            let child = fd::open_dir(parent.as_raw_fd(), &name)?;
            if fd::mount_id(parent.as_raw_fd())? != fd::mount_id(child.as_raw_fd())? {
                bail!("mount roots cannot be deleted");
            }
        } else if stat.st_mode & libc::S_IFMT != libc::S_IFREG
            && stat.st_mode & libc::S_IFMT != libc::S_IFLNK
        {
            bail!("only files, symlinks and folders can be deleted");
        }
        Ok(Self {
            parent,
            name,
            dev,
            ino,
            path,
        })
    }
    /// Check the whole tree first, so limits and boundaries fail before
    /// anything is removed rather than leaving a partially deleted folder.
    pub fn delete(self) -> Result<()> {
        let parent_dev = fd::stat(self.parent.as_raw_fd(), std::ffi::OsStr::new("."))?.st_dev;
        walk(
            self.parent.as_raw_fd(),
            &self.name,
            parent_dev,
            self.dev,
            self.ino,
            0,
            false,
        )?;
        walk(
            self.parent.as_raw_fd(),
            &self.name,
            parent_dev,
            self.dev,
            self.ino,
            0,
            true,
        )
    }
}

/// Visit a tree bottom-up, unlinking only when `unlink` is set. Refuses to
/// enter another mount or a nested Btrfs subvolume (a different st_dev on the
/// same mount), whose removal would delete a subvolume.
fn walk(
    parent: i32,
    name: &std::ffi::OsStr,
    parent_dev: u64,
    dev: u64,
    ino: u64,
    depth: usize,
    unlink: bool,
) -> Result<()> {
    if depth > 256 {
        bail!("directory nesting exceeds the deletion limit; nothing was deleted");
    }
    let stat = fd::stat(parent, name)?;
    fd::same(&stat, dev, ino)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return if unlink {
            fd::unlink(parent, name, false)
        } else {
            Ok(())
        };
    }
    if stat.st_dev != parent_dev {
        bail!("refusing to delete a subvolume or another filesystem inside the selection");
    }
    let directory = fd::open_dir(parent, name)?;
    fd::same(
        &fd::stat(directory.as_raw_fd(), std::ffi::OsStr::new("."))?,
        dev,
        ino,
    )?;
    if fd::mount_id(parent)? != fd::mount_id(directory.as_raw_fd())? {
        bail!("refusing to cross a mounted filesystem during deletion");
    }
    let path = format!("/proc/self/fd/{}", directory.as_raw_fd());
    for entry in std::fs::read_dir(path)? {
        let name = entry?.file_name();
        let child = fd::stat(directory.as_raw_fd(), &name)?;
        walk(
            directory.as_raw_fd(),
            &name,
            stat.st_dev,
            child.st_dev,
            child.st_ino,
            depth + 1,
            unlink,
        )?;
    }
    if !unlink {
        return Ok(());
    }
    fd::same(&fd::stat(parent, name)?, dev, ino)?;
    fd::unlink(parent, name, true)
}
