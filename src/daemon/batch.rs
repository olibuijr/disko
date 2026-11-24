use super::*;

/// Apply one batch of watcher changes: renames first, in order, then the
/// coalesced paths, each read from the filesystem as it is now.
pub(super) fn apply(idx: &mut Index, pending: events::Pending) -> Result<()> {
    let events::Pending { mut paths, renames } = pending;
    if paths.is_empty() && renames.is_empty() {
        return Ok(());
    }
    phase("applying changes");
    for (old, new) in renames {
        let dir = idx.rename(&old, &new)?;
        // Queued changes below a moved folder now live under its new name.
        let moved: Vec<PathBuf> = if dir {
            paths
                .keys()
                .filter(|p| p.starts_with(&old))
                .cloned()
                .collect()
        } else {
            paths
                .contains_key(&old)
                .then(|| old.clone())
                .into_iter()
                .collect()
        };
        for p in moved {
            if let (Some(r), Ok(rest)) = (paths.remove(&p), p.strip_prefix(&old)) {
                let to = if rest.as_os_str().is_empty() {
                    new.clone()
                } else {
                    new.join(rest)
                };
                paths.entry(to).and_modify(|x| *x |= r).or_insert(r);
            }
        }
        activity(idx, &new, "renamed")?;
    }
    let mut paths: Vec<_> = paths.into_iter().collect();
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    let mut recursive_parent: Option<PathBuf> = None;
    for (path, recursive) in paths {
        if recursive_parent
            .as_ref()
            .is_some_and(|p| path.starts_with(p))
        {
            continue;
        }
        idx.sync(&path, recursive)?;
        idx.update_hardlinks(&path)?;
        activity(idx, &path, if recursive { "subtree" } else { "metadata" })?;
        if recursive {
            recursive_parent = Some(path);
        }
    }
    set_meta(&idx.c, "last_event", now())?;
    idx.c.execute(
        "DELETE FROM activity WHERE id NOT IN (SELECT id FROM activity ORDER BY id DESC LIMIT 2000)",
        [],
    )?;
    idx.commit()?;
    phase("watching");
    Ok(())
}

/// Delete removed rows for up to `budget` while nothing else is waiting,
/// committing after every bounded step.
pub(super) fn collect_garbage(idx: &mut Index, stop: &AtomicBool, budget: Duration) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < budget && !stop.load(Ordering::Relaxed) {
        phase("collecting removed entries");
        let more = idx.collect_garbage(GC_ROWS)?;
        idx.commit()?;
        if !more {
            break;
        }
    }
    phase("watching");
    Ok(())
}
