use super::*;
use crate::web::lock;
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Results that are slow to compute on a busy disk (minutes, under heavy I/O)
/// but change slowly: computed on a background thread and served from memory.
struct Slot {
    value: Option<(Instant, Value)>,
    running: bool,
}

const FRESH: Duration = Duration::from_secs(15 * 60);

static CLEANUP: Mutex<Slot> = Mutex::new(Slot {
    value: None,
    running: false,
});

/// The cached cleanup list, refreshed in the background when stale. `refresh`
/// forces a new computation (after a deletion, for example).
pub(super) fn cleanup_list(db: &Path, refresh: bool) -> Value {
    serve(&CLEANUP, db, refresh, |db| {
        let c = open(db, false)?;
        c.execute_batch("PRAGMA cache_size=-16384; BEGIN")?;
        Ok(json!(cleanup(&c, 100, 64 << 20)?))
    })
}

/// Clears `running` when the background job ends, also when it panics, so a
/// failed refresh never blocks every later one.
struct Running(&'static Mutex<Slot>);
impl Drop for Running {
    fn drop(&mut self) {
        lock(self.0).running = false;
    }
}

fn serve(
    slot: &'static Mutex<Slot>,
    db: &Path,
    refresh: bool,
    compute: fn(&Path) -> Result<Value>,
) -> Value {
    let mut s = lock(slot);
    let stale = refresh || s.value.as_ref().is_none_or(|(at, _)| at.elapsed() > FRESH);
    if stale && !s.running {
        s.running = true;
        let db: PathBuf = db.to_owned();
        std::thread::spawn(move || {
            let _running = Running(slot);
            let result = compute(&db);
            let mut s = lock(slot);
            match result {
                Ok(rows) => s.value = Some((Instant::now(), rows)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(slot: &Mutex<Slot>) {
        let start = Instant::now();
        while lock(slot).running {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "job never finished"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_panicking_refresh_does_not_wedge_the_cache() {
        static SLOT: Mutex<Slot> = Mutex::new(Slot {
            value: None,
            running: false,
        });
        let db = Path::new("unused.db");
        let v = serve(&SLOT, db, false, |_| panic!("injected failure"));
        assert_eq!(v["updating"], true);
        settle(&SLOT);
        // The next request starts a new job and its result is served.
        serve(&SLOT, db, true, |_| Ok(json!(["rows"])));
        settle(&SLOT);
        let v = serve(&SLOT, db, false, |_| unreachable!("fresh value"));
        assert_eq!(v["folders"], json!(["rows"]));
        assert_eq!(v["updating"], false);
    }
}
