use crate::{
    cli::{Cli, ScanArgs},
    index::*,
    mounts, web,
};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use notify::{
    Event, EventKind, Watcher,
    event::{AccessKind, AccessMode, ModifyKind},
};
use rusqlite::params;
use serde_json::json;
use std::{
    collections::HashMap,
    fs::OpenOptions,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
mod config;
mod events;
mod fanotify;
mod queue;
use events::ignored_event;

pub struct WatchOptions {
    pub interval: u64,
    pub debounce: u64,
    pub web: Option<web::Options>,
}

pub fn writer(cli: &Cli, s: &ScanArgs, watch: Option<WatchOptions>) -> Result<()> {
    let (mut idx, _lock, db) = config::prepare(cli, s)?;
    let daemon = watch.is_some();
    let result = run(&mut idx, db, watch);
    // SIGTERM/SIGINT abort a running scan through an error. For the daemon that
    // is an orderly stop (completed batches are committed when `idx` drops), so
    // systemd should not record it as a failure. One-shot `scan` still fails.
    match result {
        Err(e) if daemon && idx.stop.load(Ordering::Relaxed) => {
            eprintln!("disko: stopped ({e:#})");
            set_meta(&idx.c, "state", "stopped")?;
            set_meta(&idx.c, "pid", 0)?;
            Ok(())
        }
        other => other,
    }
}

/// Below this much free space on the database's filesystem, full scans wait;
/// a scan that hits a full disk fails and costs a restart plus a rescan.
const MIN_FREE_BYTES: u64 = 1 << 30;

fn low_space(db: &Path) -> bool {
    db.parent()
        .and_then(|p| fs2::available_space(p).ok())
        .is_some_and(|free| free < MIN_FREE_BYTES)
}

fn run(idx: &mut Index, db: PathBuf, watch: Option<WatchOptions>) -> Result<()> {
    let stop = idx.stop.clone();
    let rescan = Arc::new(AtomicBool::new(false));
    if let Some(options) = watch.as_ref().and_then(|w| w.web.as_ref()) {
        idx.commit()?;
        web::start(
            db.clone(),
            options.clone(),
            stop.clone(),
            Some(rescan.clone()),
        )?;
    }
    let mut table = mounts::read();
    idx.set_skip(mounts::duplicates(&table))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel::<notify::Result<Event>>(65536);
    let mut fan = None;
    if watch.is_some() {
        match fanotify::start(
            idx.roots.clone(),
            idx.excludes.clone(),
            &table,
            tx.clone(),
            overflow.clone(),
            stop.clone(),
        ) {
            Ok((f, devices)) => {
                idx.filesystem_devices = devices;
                fan = Some(f);
            }
            Err(e) => eprintln!("fanotify unavailable ({e}); using directory inotify watches"),
        }
        let overflow = overflow.clone();
        let w = notify::RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                if event.as_ref().is_ok_and(|e| ignored_event(&e.kind)) {
                    return;
                }
                if tx.try_send(event).is_err() {
                    overflow.store(true, Ordering::Relaxed);
                }
            },
            notify::Config::default(),
        )?;
        idx.watcher = Some(w);
    }
    let queue = queue::EventQueue::start(
        rx,
        idx.roots.clone(),
        idx.excludes.clone(),
        overflow.clone(),
        stop.clone(),
    );
    let Some(options) = watch else {
        set_meta(&idx.c, "recovery_reason", "one-shot scan")?;
        idx.reconcile()?;
        set_meta(&idx.c, "pid", 0)?;
        return Ok(());
    };
    let (interval, debounce) = (options.interval, options.debounce);
    let mut last_scan = Instant::now();
    let mut last_health = Instant::now();
    let mut last_mounts = Instant::now();
    let mut due = true; // The startup reconciliation.
    let mut waiting = false;
    let mut started = false;
    while !stop.load(Ordering::Relaxed) {
        let requested = rescan.swap(false, Ordering::Relaxed);
        due |= requested
            || overflow.swap(false, Ordering::Relaxed)
            || (interval > 0 && last_scan.elapsed() >= Duration::from_secs(interval));
        if due && low_space(&db) {
            if !waiting {
                eprintln!(
                    "less than {} MiB free for the index; full scan postponed",
                    MIN_FREE_BYTES >> 20
                );
                set_meta(&idx.c, "state", "waiting for disk space")?;
                idx.commit()?;
                waiting = true;
            }
        } else if due {
            let reason = if !started {
                "startup"
            } else if requested {
                "dashboard request"
            } else {
                "event overflow, watcher error, or scheduled reconciliation"
            };
            set_meta(&idx.c, "recovery_reason", reason)?;
            started = true;
            waiting = false;
            due = false;
            idx.reconcile()?;
            last_scan = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(debounce.clamp(10, 2000)));
        if last_mounts.elapsed() >= Duration::from_secs(2) {
            last_mounts = Instant::now();
            let current = mounts::read();
            if current != table {
                remount(idx, fan.as_ref(), &table, &current)?;
                table = current;
            }
        }
        let mut pending = queue.take();
        if !pending.is_empty() {
            let mut paths: Vec<_> = pending.drain().collect();
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
            idx.c.execute("DELETE FROM activity WHERE id NOT IN (SELECT id FROM activity ORDER BY id DESC LIMIT 2000)",[])?;
            idx.commit()?;
        }
        if last_health.elapsed() > Duration::from_secs(10) {
            idx.commit()?;
            last_health = Instant::now();
        }
    }
    set_meta(&idx.c, "state", "stopped")?;
    set_meta(&idx.c, "pid", 0)?;
    idx.commit()?;
    Ok(())
}

fn activity(idx: &Index, path: &Path, kind: &str) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    idx.c.execute(
        "INSERT INTO activity(ts,path,kind) VALUES(?1,?2,?3)",
        params![now(), path.as_os_str().as_bytes(), kind],
    )?;
    Ok(())
}

/// Something was mounted or unmounted: mark new filesystems, update skipped
/// duplicates, and resync each changed mountpoint, which now shows either the
/// new filesystem or the directory underneath.
fn remount(
    idx: &mut Index,
    fan: Option<&fanotify::Fanotify>,
    before: &[mounts::Mount],
    after: &[mounts::Mount],
) -> Result<()> {
    if let Some(fan) = fan {
        idx.filesystem_devices = fan.refresh(after);
    }
    idx.set_skip(mounts::duplicates(after))?;
    let changed: Vec<(PathBuf, &str)> = after
        .iter()
        .filter(|m| !before.iter().any(|b| b.id == m.id))
        .map(|m| (m.point.clone(), "mount"))
        .chain(
            before
                .iter()
                .filter(|b| !after.iter().any(|m| m.id == b.id))
                .map(|b| (b.point.clone(), "unmount")),
        )
        .collect();
    for (point, kind) in changed {
        if !idx.relevant(&point) {
            continue;
        }
        idx.forget_watches(&point);
        idx.sync(&point, true)?;
        activity(idx, &point, kind)?;
    }
    idx.commit()
}
