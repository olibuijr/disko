mod decode;
mod mounts;
use anyhow::{Result, bail};
use std::{
    collections::HashSet,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
};

pub(super) fn start(
    roots: Vec<PathBuf>,
    excludes: Vec<PathBuf>,
    sender: SyncSender<notify::Result<notify::Event>>,
    overflow: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Result<HashSet<u64>> {
    let raw = unsafe {
        libc::fanotify_init(
            libc::FAN_CLOEXEC | libc::FAN_NONBLOCK | libc::FAN_REPORT_DFID_NAME,
            libc::O_RDONLY as u32 | libc::O_CLOEXEC as u32,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let (handles, devices) = mounts::mark(fd.as_raw_fd(), &roots, &excludes)?;
    if devices.is_empty() {
        bail!("no filesystems support fanotify");
    }
    eprintln!(
        "fanotify covers {} device IDs with {} filesystem handle groups",
        devices.len(),
        handles.len()
    );
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
            decode::events(
                &buffer[..count as usize],
                &handles,
                &roots,
                &excludes,
                &sender,
                &overflow,
            );
        }
    });
    Ok(devices)
}
