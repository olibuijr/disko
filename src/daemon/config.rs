use super::*;
pub(super) fn prepare(cli: &Cli, s: &ScanArgs) -> Result<(Index, std::fs::File, PathBuf)> {
    let db = absolute(&cli.db)?;
    std::fs::create_dir_all(db.parent().context("database needs a parent directory")?)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(db.with_extension("lock.writer"))?;
    lock.try_lock_exclusive()
        .context("another Disko writer is running; queries can run concurrently")?;
    let roots = s
        .roots
        .iter()
        .map(|p| absolute(p))
        .collect::<Result<Vec<_>>>()?;
    for root in &roots {
        if !root.is_dir() {
            bail!(
                "index root {} must be an existing directory",
                root.display()
            );
        }
    }
    let mut roots = roots;
    roots.sort();
    roots.dedup();
    let all = roots.clone();
    roots.retain(|p| !all.iter().any(|other| other != p && p.starts_with(other)));
    let mut excludes = vec![
        PathBuf::from("/proc"),
        PathBuf::from("/sys"),
        PathBuf::from("/dev"),
        PathBuf::from("/run"),
        db.parent().unwrap().to_owned(),
    ];
    for p in &s.exclude {
        excludes.push(absolute(p)?);
    }
    for root in &roots {
        if excludes.iter().any(|e| root.starts_with(e)) {
            bail!(
                "root {} is excluded (place the database outside indexed roots)",
                root.display()
            );
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    let c = open(&db, true)?;
    // Roots define the tree; changing them needs a fresh index. Exclusions can
    // change in place: newly excluded trees are pruned below, and newly
    // included ones are picked up by the startup scan.
    let coverage = json!({"roots":roots,"excludes":excludes});
    let previous: Option<serde_json::Value> = c
        .query_row("SELECT value FROM meta WHERE key='coverage'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok());
    let mut pruned = Vec::new();
    if let Some(previous) = previous {
        if previous["roots"] != coverage["roots"] {
            bail!(
                "index roots differ from the existing index; use a new --db path to change roots"
            );
        }
        let before: Vec<PathBuf> =
            serde_json::from_value(previous["excludes"].clone()).unwrap_or_default();
        pruned = excludes
            .iter()
            .filter(|e| !before.iter().any(|b| e.starts_with(b)))
            .cloned()
            .collect();
        if before != excludes {
            // Newly included trees are only found by a scan from the roots.
            c.execute("DELETE FROM scan_queue", [])?;
            eprintln!(
                "coverage: exclusions changed; pruning {} newly excluded trees",
                pruned.len()
            );
        }
    }
    set_meta(&c, "pid", std::process::id())?;
    c.execute(
        "UPDATE scans SET finished=?1,state='interrupted' WHERE finished IS NULL",
        [now()],
    )?;
    let mut idx = Index::new(c, roots, excludes, stop.clone(), s.jobs)?;
    for path in pruned {
        idx.forget_path(&path)?;
    }
    set_meta(&idx.c, "coverage", coverage.to_string())?;
    idx.commit()?;
    Ok((idx, lock, db))
}
