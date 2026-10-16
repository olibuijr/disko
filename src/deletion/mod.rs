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
    pub fn delete(self) -> Result<()> {
        remove(self.parent.as_raw_fd(), &self.name, self.dev, self.ino, 0)
    }
}

fn remove(parent: i32, name: &std::ffi::OsStr, dev: u64, ino: u64, depth: usize) -> Result<()> {
    if depth > 256 {
        bail!("directory nesting exceeds the deletion limit");
    }
    let stat = fd::stat(parent, name)?;
    fd::same(&stat, dev, ino)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return fd::unlink(parent, name, false);
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
        remove(
            directory.as_raw_fd(),
            &name,
            child.st_dev as u64,
            child.st_ino as u64,
            depth + 1,
        )?;
    }
    fd::same(&fd::stat(parent, name)?, dev, ino)?;
    fd::unlink(parent, name, true)
}
