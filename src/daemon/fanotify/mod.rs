mod decode;
mod mounts;
use crate::mounts::Mount;
use anyhow::{Result, bail};
use std::{
    collections::HashSet,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
};

/// Filesystem-wide marks, refreshable when the mount table changes.
pub(super) struct Fanotify {
    fd: Arc<OwnedFd>,
    sources: Arc<Mutex<mounts::Sources>>,
    roots: Vec<PathBuf>,
    excludes: Vec<PathBuf>,
}

impl Fanotify {
    /// Mark new filesystems and drop unmounted ones; returns covered device IDs.
    pub(super) fn refresh(&self, mounts: &[Mount]) -> HashSet<u64> {
        self.sources.lock().unwrap().refresh(
            self.fd.as_raw_fd(),
            mounts,
            &self.roots,
            &self.excludes,
        )
    }
}

pub(super) fn start(
    roots: Vec<PathBuf>,
    excludes: Vec<PathBuf>,
    mounts: &[Mount],
    sender: SyncSender<notify::Result<notify::Event>>,
    overflow: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Result<(Fanotify, HashSet<u64>)> {
    let raw = unsafe {
        libc::fanotify_init(
            libc::FAN_CLOEXEC | libc::FAN_NONBLOCK | libc::FAN_REPORT_DFID_NAME,
            libc::O_RDONLY as u32 | libc::O_CLOEXEC as u32,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let fan = Fanotify {
        fd: Arc::new(unsafe { OwnedFd::from_raw_fd(raw) }),
        sources: Arc::default(),
        roots: roots.clone(),
        excludes: excludes.clone(),
    };
    let devices = fan.refresh(mounts);
    let marked = fan.sources.lock().unwrap().len();
    if marked == 0 {
        bail!("no filesystems support fanotify");
    }
    eprintln!(
        "fanotify covers {} device IDs on {marked} filesystems",
        devices.len()
    );
    let (fd, sources) = (fan.fd.clone(), fan.sources.clone());
    std::thread::spawn(move || {
        let mut buffer = vec![0_u8; 256 * 1024];
        while !stop.load(Ordering::Relaxed) {
            let mut poll = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, 500) } <= 0 {
                continue;
            }
            let count =
                unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
            if count < 0 {
                if io::Error::last_os_error().kind() != io::ErrorKind::WouldBlock {
                    overflow.store(true, Ordering::Relaxed);
                }
                continue;
            }
            let sources = sources.lock().unwrap();
            if sources.is_empty() {
                continue;
            }
            decode::events(
                &buffer[..count as usize],
                &sources,
                &roots,
                &excludes,
                &sender,
                &overflow,
            );
        }
    });
    Ok((fan, devices))
}
