use super::*;
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Results that are slow to compute on a busy disk (minutes, under heavy I/O)
/// but change slowly: computed on a background thread and served from memory.
struct Slot {
    value: Option<(Instant, Value)>,
    running: bool,
}

const FRESH: Duration = Duration::from_secs(15 * 60);

fn slot() -> &'static Mutex<Slot> {
    static SLOT: OnceLock<Mutex<Slot>> = OnceLock::new();
    SLOT.get_or_init(|| {
        Mutex::new(Slot {
            value: None,
            running: false,
        })
    })
}

/// The cached cleanup list, refreshed in the background when stale. `refresh`
/// forces a new computation (after a deletion, for example).
pub(super) fn cleanup_list(db: &Path, refresh: bool) -> Value {
    let mut s = slot().lock().unwrap();
    let stale = refresh || s.value.as_ref().is_none_or(|(at, _)| at.elapsed() > FRESH);
    if stale && !s.running {
        s.running = true;
        let db: PathBuf = db.to_owned();
        std::thread::spawn(move || {
            let result = open(&db, false).and_then(|c| {
                c.execute_batch("PRAGMA cache_size=-16384; BEGIN")?;
                cleanup(&c, 100, 64 << 20)
            });
            let mut s = slot().lock().unwrap();
            s.running = false;
            match result {
                Ok(rows) => s.value = Some((Instant::now(), json!(rows))),
                Err(e) => eprintln!("cleanup suggestions: {e:#}"),
            }
        });
    }
    match &s.value {
        Some((at, rows)) => {
            json!({"folders": rows, "age_seconds": at.elapsed().as_secs(), "updating": s.running})
        }
        None => json!({"folders": [], "age_seconds": null, "updating": s.running}),
    }
}
