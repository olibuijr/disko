use crate::{
    cli::{Cli, ScanArgs},
    index::*,
    web,
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
    path::PathBuf,
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
    let overflow = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel::<notify::Result<Event>>(65536);
    if watch.is_some() {
        match fanotify::start(
            idx.roots.clone(),
            idx.excludes.clone(),
            tx.clone(),
            overflow.clone(),
            stop.clone(),
        ) {
            Ok(devices) => idx.filesystem_devices = devices,
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
    idx.reconcile()?;
    let Some(options) = watch else {
        set_meta(&idx.c, "pid", 0)?;
        return Ok(());
    };
    let (interval, debounce) = (options.interval, options.debounce);
    let mut last_scan = Instant::now();
    let mut last_health = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let requested = rescan.swap(false, Ordering::Relaxed);
        if requested
            || overflow.swap(false, Ordering::Relaxed)
            || (interval > 0 && last_scan.elapsed() >= Duration::from_secs(interval))
        {
            set_meta(
                &idx.c,
                "recovery_reason",
                if requested {
                    "dashboard request"
                } else {
                    "event overflow, watcher error, or scheduled reconciliation"
                },
            )?;
            idx.reconcile()?;
            last_scan = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(debounce.clamp(10, 2000)));
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
                use std::os::unix::ffi::OsStrExt;
                idx.c.execute(
                    "INSERT INTO activity(ts,path,kind) VALUES(?1,?2,?3)",
                    params![
                        now(),
                        path.as_os_str().as_bytes(),
                        if recursive { "subtree" } else { "metadata" }
                    ],
                )?;
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
