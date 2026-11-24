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

const DFID_NAME: u8 = libc::FAN_EVENT_INFO_TYPE_DFID_NAME;
/// `FAN_RENAME` records: the old and the new parent handle and name.
const OLD_DFID_NAME: u8 = 10;
const NEW_DFID_NAME: u8 = 12;

/// The path named by one directory-handle-plus-name info record.
fn path_in(record: &[u8], sources: &Sources) -> Option<PathBuf> {
    let fsid = record[4..12].try_into().unwrap();
    let handle_size = u32::from_ne_bytes(record[12..16].try_into().unwrap()) as usize;
    let name_start = 20 + handle_size;
    if name_start >= record.len() || handle_size > 128 {
        return None;
    }
    let parent = sources.resolve(fsid, &record[12..name_start])?;
    let name = &record[name_start..];
    let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
    absolute(&parent.join(std::ffi::OsStr::from_bytes(&name[..end]))).ok()
}

pub(super) fn events(
    buffer: &[u8],
    sources: &Sources,
    roots: &[PathBuf],
    excludes: &[PathBuf],
    sender: &SyncSender<notify::Result<notify::Event>>,
    overflow: &AtomicBool,
) {
    let wanted = |path: &PathBuf| {
        !(excludes.iter().any(|e| path.starts_with(e))
            || path.components().any(|c| c.as_os_str() == ".snapshots"))
            && roots.iter().any(|r| path.starts_with(r))
    };
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
        let (mut old, mut new, mut plain) = (None, None, Vec::new());
        while record + 20 <= offset + len {
            let size =
                u16::from_ne_bytes(buffer[record + 2..record + 4].try_into().unwrap()) as usize;
            if size < 20 || record + size > offset + len {
                overflow.store(true, Ordering::Relaxed);
                break;
            }
            let kind = buffer[record];
            if matches!(kind, DFID_NAME | OLD_DFID_NAME | NEW_DFID_NAME)
                && let Some(path) = path_in(&buffer[record..record + size], sources)
                && wanted(&path)
            {
                match kind {
                    OLD_DFID_NAME => old = Some(path),
                    NEW_DFID_NAME => new = Some(path),
                    _ => plain.push(path),
                }
            }
            record += size;
        }
        let send = |kind, paths: Vec<PathBuf>| {
            let mut event = notify::Event::new(kind);
            event.paths = paths;
            if sender.try_send(Ok(event)).is_err() {
                overflow.store(true, Ordering::Relaxed);
            }
        };
        use notify::EventKind::{Create, Modify, Remove};
        use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
        match (old, new) {
            (Some(o), Some(n)) => send(Modify(ModifyKind::Name(RenameMode::Both)), vec![o, n]),
            (Some(o), None) => send(Remove(RemoveKind::Any), vec![o]),
            (None, Some(n)) => send(Create(CreateKind::Any), vec![n]),
            (None, None) => {}
        }
        for path in plain {
            let kind = if meta.mask & (libc::FAN_CREATE | libc::FAN_MOVED_TO) != 0 {
                Create(CreateKind::Any)
            } else if meta.mask & (libc::FAN_DELETE | libc::FAN_MOVED_FROM) != 0 {
                Remove(RemoveKind::Any)
            } else {
                Modify(ModifyKind::Any)
            };
            send(kind, vec![path]);
        }
        offset += len;
    }
}
