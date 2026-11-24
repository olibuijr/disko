use super::*;

/// With no commit or walker progress for this long, the writer is stuck (for
/// example a single statement starved by I/O limits). Exiting lets systemd
/// restart it, and the scan resumes from its saved frontier.
const STALL: i64 = 15 * 60;
/// After SIGTERM, give the writer this long to commit and exit on its own.
const STOP_GRACE: Duration = Duration::from_secs(20);

pub(super) fn start(stop: Arc<AtomicBool>) {
    progress();
    std::thread::Builder::new()
        .name("disko-watchdog".into())
        .spawn(move || {
            let mut stopping: Option<Instant> = None;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let phase = PHASE.lock().map(|p| *p).unwrap_or("unknown");
                if stop.load(Ordering::Relaxed) {
                    let since = *stopping.get_or_insert_with(Instant::now);
                    if since.elapsed() > STOP_GRACE {
                        eprintln!("disko: still {phase} {}s after stop was requested; exiting", STOP_GRACE.as_secs());
                        std::process::exit(0);
                    }
                    continue;
                }
                let idle = now() - PROGRESS.load(Ordering::Relaxed);
                if idle > STALL {
                    eprintln!("disko: no progress for {idle}s while {phase}; exiting so the service restarts");
                    std::process::exit(1);
                }
            }
        })
        .expect("watchdog thread");
}
