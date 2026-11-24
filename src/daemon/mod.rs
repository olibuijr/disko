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
mod batch;
mod config;
mod events;
mod fanotify;
mod queue;
mod remount;
mod scan_mode;
mod watchdog;
use events::ignored_event;
use remount::{MIN_FREE_BYTES, activity, low_space, remount};

pub struct WatchOptions {
    pub interval: u64,
    /// Seconds between deep scans; the scheduled scans in between are incremental.
    pub deep_interval: u64,
    pub debounce: u64,
    pub web: Option<web::Options>,
}

pub fn writer(cli: &Cli, s: &ScanArgs, watch: Option<WatchOptions>, deep: bool) -> Result<()> {
    let (mut idx, _lock, db) = config::prepare(cli, s)?;
    let daemon = watch.is_some();
    if daemon {
        watchdog::start(idx.stop.clone());
    }
    let result = run(&mut idx, db, watch, deep);
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

fn run(idx: &mut Index, db: PathBuf, watch: Option<WatchOptions>, deep: bool) -> Result<()> {
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
        idx.reconcile(deep)?;
        while idx.collect_garbage(GC_ROWS)? {
            idx.commit()?;
        }
        set_meta(&idx.c, "pid", 0)?;
        return Ok(());
    };
    let (interval, debounce) = (options.interval, options.debounce);
    let mut deep_due = scan_mode::deep_due(idx, options.deep_interval)?;
    let mut last_scan = Instant::now();
    let mut last_health = Instant::now();
    let mut last_mounts = Instant::now();
    let mut due = true; // The startup reconciliation.
    let mut waiting = false;
    let mut started = false;
    while !stop.load(Ordering::Relaxed) {
        let requested = rescan.swap(false, Ordering::Relaxed);
        let lost = overflow.swap(false, Ordering::Relaxed);
        // Lost events or a manual request: anything may have changed, so
        // stat everything. A schedule tick is incremental unless a deep scan
        // is due.
        deep_due |= requested || lost;
        if interval > 0 && last_scan.elapsed() >= Duration::from_secs(interval) {
            due = true;
            deep_due |= scan_mode::deep_due(idx, options.deep_interval)?;
        }
        due |= requested || lost;
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
            idx.reconcile(deep_due)?;
            deep_due = false;
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
        let pending = queue.take();
        let idle = pending.paths.is_empty() && pending.renames.is_empty();
        batch::apply(idx, pending)?;
        if idle && !due {
            batch::collect_garbage(idx, &stop, Duration::from_millis(500))?;
        }
        if last_health.elapsed() > Duration::from_secs(10) {
            if !due {
                idx.record_growth()?;
            }
            idx.commit()?;
            last_health = Instant::now();
        }
    }
    set_meta(&idx.c, "state", "stopped")?;
    set_meta(&idx.c, "pid", 0)?;
    idx.commit()?;
    Ok(())
}
