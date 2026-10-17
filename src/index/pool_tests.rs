use super::*;
use std::time::Instant;

fn tree(dirs: usize) -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for i in 0..dirs {
        fs::create_dir_all(t.path().join(format!("{}/{i}", i % 10))).unwrap();
    }
    t
}

fn job(path: &Path) -> walk::Job {
    walk::Job {
        path: path.to_owned(),
        id: 1,
        seen: 1,
        place: walk::Place::Regular,
    }
}

#[test]
fn interrupted_receive_returns_instead_of_hanging() {
    let t = tree(50);
    let stop = Arc::new(AtomicBool::new(false));
    let filter = Arc::new(walk::Filter::new(Vec::new()));
    let pool = Pool::start(stop.clone(), filter, 4, job(t.path())).unwrap();
    assert!(pool.receive().is_ok());
    stop.store(true, Ordering::Relaxed);
    // Nothing else was dispatched, so only the stop flag can end this wait.
    let started = Instant::now();
    let error = match pool.receive() {
        Err(e) => e.to_string(),
        Ok(_) => panic!("received a result after stop"),
    };
    assert!(error.contains("interrupted"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn dropping_an_abandoned_pool_skips_its_backlog() {
    let t = tree(4000);
    let stop = Arc::new(AtomicBool::new(false));
    let filter = Arc::new(walk::Filter::new(Vec::new()));
    let mut pool = Pool::start(stop, filter, 2, job(t.path())).unwrap();
    pool.receive().unwrap();
    for i in 0..10 {
        for _ in 0..200 {
            pool.dispatch(job(&t.path().join(i.to_string())));
        }
    }
    let backlog = pool.shared.jobs.lock().unwrap().len();
    assert!(backlog > 1000, "backlog {backlog}");
    // What Drop does first. Workers check `halt` before taking each job, so
    // at most one in-flight job per worker leaves the queue afterwards.
    pool.shared.halt.store(true, Ordering::Relaxed);
    pool.shared.signal.notify_all();
    thread::sleep(Duration::from_millis(300));
    while pool.rx.try_recv().is_ok() {}
    let left = pool.shared.jobs.lock().unwrap().len();
    assert!(
        left + pool.crew.len() >= backlog,
        "{left} of {backlog} left"
    );
    drop(pool);
}
