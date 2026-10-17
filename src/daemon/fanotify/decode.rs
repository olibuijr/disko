use super::mounts::Sources;
use crate::index::absolute;
use std::{
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
};

pub(super) fn events(
    buffer: &[u8],
    sources: &Sources,
    roots: &[PathBuf],
    excludes: &[PathBuf],
    sender: &SyncSender<notify::Result<notify::Event>>,
    overflow: &AtomicBool,
) {
    let mut offset = 0;
    while offset + 24 <= buffer.len() {
        let meta = unsafe {
            std::ptr::read_unaligned(
                buffer[offset..]
                    .as_ptr()
                    .cast::<libc::fanotify_event_metadata>(),
            )
        };
        let len = meta.event_len as usize;
        if len < 24
            || meta.metadata_len < 24
            || meta.metadata_len as usize > len
            || offset + len > buffer.len()
            || meta.vers != libc::FANOTIFY_METADATA_VERSION
        {
            overflow.store(true, Ordering::Relaxed);
            break;
        }
        if meta.mask & libc::FAN_Q_OVERFLOW != 0 {
            overflow.store(true, Ordering::Relaxed);
        }
        let mut record = offset + meta.metadata_len as usize;
        while record + 20 <= offset + len {
            let size =
                u16::from_ne_bytes(buffer[record + 2..record + 4].try_into().unwrap()) as usize;
            if size < 20 || record + size > offset + len {
                overflow.store(true, Ordering::Relaxed);
                break;
            }
            if buffer[record] == libc::FAN_EVENT_INFO_TYPE_DFID_NAME {
                let fsid = buffer[record + 4..record + 12].try_into().unwrap();
                let handle_size =
                    u32::from_ne_bytes(buffer[record + 12..record + 16].try_into().unwrap())
                        as usize;
                let name_start = record + 20 + handle_size;
                if name_start < record + size
                    && handle_size <= 128
                    && let Some(parent) = sources.resolve(fsid, &buffer[record + 12..name_start])
                {
                    let name = &buffer[name_start..record + size];
                    let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
                    if let Ok(path) =
                        absolute(&parent.join(std::ffi::OsStr::from_bytes(&name[..end])))
                    {
                        let excluded = excludes.iter().any(|e| path.starts_with(e))
                            || path.components().any(|c| c.as_os_str() == ".snapshots");
                        if !excluded && roots.iter().any(|r| path.starts_with(r)) {
                            let kind = if meta.mask & (libc::FAN_CREATE | libc::FAN_MOVED_TO) != 0 {
                                notify::EventKind::Create(notify::event::CreateKind::Any)
                            } else if meta.mask & (libc::FAN_DELETE | libc::FAN_MOVED_FROM) != 0 {
                                notify::EventKind::Remove(notify::event::RemoveKind::Any)
                            } else {
                                notify::EventKind::Modify(notify::event::ModifyKind::Any)
                            };
                            if sender
                                .try_send(Ok(notify::Event::new(kind).add_path(path)))
                                .is_err()
                            {
                                overflow.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            record += size;
        }
        offset += len;
    }
}
