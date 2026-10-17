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
                    add(pending, p, recursive, roots, PENDING_LIMIT);
                }
            }
        }
        Err(e) => {
            eprintln!("watcher: {e}; scheduling reconciliation");
            overflow.store(true, Ordering::Relaxed);
        }
    }
}

const PENDING_LIMIT: usize = 65536;

/// Queue a changed path. When the queue is full, merge entries into their
/// parent directories (as recursive resyncs) until it is half full, rather
/// than dropping events and forcing a rescan of every root. This happens while
/// a long scan keeps the writer busy and a build or sync touches many files.
fn add(
    pending: &mut HashMap<PathBuf, bool>,
    p: PathBuf,
    recursive: bool,
    roots: &[PathBuf],
    limit: usize,
) {
    if pending.len() >= limit && !pending.contains_key(&p) {
        while pending.len() > limit / 2 {
            let before = pending.len();
            for (path, r) in std::mem::take(pending) {
                let (path, r) = match path.parent() {
                    Some(up) if !roots.contains(&path) => (up.to_owned(), true),
                    _ => (path, r),
                };
                pending.entry(path).and_modify(|x| *x |= r).or_insert(r);
            }
            if pending.len() == before {
                break; // Everything is already at a root.
            }
        }
    }
    pending
        .entry(p)
        .and_modify(|r| *r |= recursive)
        .or_insert(recursive);
}

pub(super) fn ignored_event(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Access(_))
        && !matches!(
            kind,
            EventKind::Access(AccessKind::Close(AccessMode::Write))
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_queue_merges_into_recursive_parents_within_roots() {
        let roots = vec![PathBuf::from("/data")];
        let mut pending = HashMap::new();
        for a in 0..4 {
            for b in 0..4 {
                add(
                    &mut pending,
                    format!("/data/{a}/{b}").into(),
                    false,
                    &roots,
                    16,
                );
            }
        }
        assert_eq!(pending.len(), 16);
        add(&mut pending, "/data/9/9".into(), false, &roots, 16);
        assert!(pending.len() <= 9, "{pending:?}");
        for a in 0..4 {
            assert_eq!(pending.get(Path::new(&format!("/data/{a}"))), Some(&true));
        }
        assert_eq!(pending.get(Path::new("/data/9/9")), Some(&false));
        // Never coarsens above a root.
        let mut flat = HashMap::new();
        for a in 0..8 {
            add(&mut flat, format!("/data/{a}").into(), false, &roots, 4);
        }
        assert!(flat.keys().all(|p| p.starts_with("/data")));
    }
}
