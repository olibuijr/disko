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
    // Changing coverage in an existing database could leave misleading old totals.
    let coverage = serde_json::to_string(&json!({"roots":roots,"excludes":excludes}))?;
    let previous: Option<String> = c
        .query_row("SELECT value FROM meta WHERE key='coverage'", [], |r| {
            r.get(0)
        })
        .ok();
    if previous.as_ref().is_some_and(|v| v != &coverage) {
        bail!(
            "coverage differs from the existing index; use a new --db path to change roots/exclusions"
        );
    }
    set_meta(&c, "coverage", coverage)?;
    set_meta(&c, "pid", std::process::id())?;
    c.execute(
        "UPDATE scans SET finished=?1,state='interrupted' WHERE finished IS NULL",
        [now()],
    )?;
    let idx = Index::new(c, roots, excludes, stop.clone())?;
    Ok((idx, lock, db))
}
