use super::*;

pub(super) fn collect(
    event: notify::Result<Event>,
    roots: &[PathBuf],
    excludes: &[PathBuf],
    pending: &mut HashMap<PathBuf, bool>,
    overflow: &AtomicBool,
) {
    match event {
        Ok(e) => {
            if e.need_rescan() {
                overflow.store(true, Ordering::Relaxed);
            }
            if ignored_event(&e.kind) {
                return;
            }
            let recursive = matches!(
                e.kind,
                EventKind::Create(_)
                    | EventKind::Modify(ModifyKind::Name(_))
                    | EventKind::Any
                    | EventKind::Other
            );
            for p in e.paths {
                if roots.iter().any(|r| p.starts_with(r))
                    && !excludes.iter().any(|r| p.starts_with(r))
                    && !p.components().any(|c| c.as_os_str() == ".snapshots")
                {
                    if pending.len() >= 65536 && !pending.contains_key(&p) {
                        overflow.store(true, Ordering::Relaxed);
                        continue;
                    }
                    pending
                        .entry(p)
                        .and_modify(|r| *r |= recursive)
                        .or_insert(recursive);
                }
            }
        }
        Err(e) => {
            eprintln!("watcher: {e}; scheduling reconciliation");
            overflow.store(true, Ordering::Relaxed);
        }
    }
}

pub(super) fn ignored_event(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Access(_))
        && !matches!(
            kind,
            EventKind::Access(AccessKind::Close(AccessMode::Write))
        )
}
