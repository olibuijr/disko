use super::*;
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    thread::JoinHandle,
};

/// Pending directories, taken depth-first so the backlog stays near the
/// siblings of the paths being explored instead of a whole tree level.
#[derive(Default)]
struct Queued {
    front: Vec<walk::Job>,
    main: Vec<walk::Job>,
    tail: Vec<walk::Job>,
}

impl Queued {
    fn push(&mut self, job: walk::Job) {
        match job.place {
            walk::Place::Front => self.front.push(job),
            walk::Place::Regular => self.main.push(job),
            walk::Place::Tail => self.tail.push(job),
        }
    }
    fn pop(&mut self) -> Option<walk::Job> {
        self.front
            .pop()
            .or_else(|| self.main.pop())
            .or_else(|| self.tail.pop())
    }
    fn len(&self) -> usize {
        self.front.len() + self.main.len() + self.tail.len()
    }
}

struct Shared {
    jobs: Mutex<Queued>,
    signal: Condvar,
    halt: AtomicBool,
    stop: Arc<AtomicBool>,
    filter: Arc<walk::Filter>,
}

pub(super) struct Pool {
    shared: Arc<Shared>,
    rx: mpsc::Receiver<walk::DirResult>,
    tx: Option<mpsc::SyncSender<walk::DirResult>>,
    crew: Vec<JoinHandle<()>>,
    limit: usize,
}

impl Pool {
    /// Start with one worker; `dispatch` adds more, up to the configured crew
    /// size, only while queued directories outnumber workers. Event-driven syncs
    /// of small new directories therefore never spawn a full crew.
    pub(super) fn start(
        stop: Arc<AtomicBool>,
        filter: Arc<walk::Filter>,
        count: u32,
        root: walk::Job,
    ) -> Result<Pool> {
        let (tx, rx) = mpsc::sync_channel(128);
        let mut jobs = Queued::default();
        jobs.push(root);
        let mut pool = Pool {
            shared: Arc::new(Shared {
                jobs: Mutex::new(jobs),
                signal: Condvar::new(),
                halt: AtomicBool::new(false),
                stop,
                filter,
            }),
            rx,
            tx: Some(tx),
            crew: Vec::new(),
            limit: walk::crew_size(count),
        };
        if let Err(e) = pool.spawn() {
            bail!("no scan worker: {e}");
        }
        Ok(pool)
    }

    fn spawn(&mut self) -> std::io::Result<()> {
        let (shared, tx) = (self.shared.clone(), self.tx.clone().unwrap());
        let handle = thread::Builder::new()
            .name("disko-walk".into())
            .spawn(move || walk_loop(&shared, &tx))?;
        self.crew.push(handle);
        Ok(())
    }

    pub(super) fn dispatch(&mut self, job: walk::Job) {
        let backlog = {
            let mut q = self.shared.jobs.lock().unwrap();
            q.push(job);
            q.len()
        };
        self.shared.signal.notify_one();
        if self.crew.len() < self.limit && backlog > self.crew.len() {
            let _ = self.spawn(); // Fewer workers only slows the walk.
        }
    }

    /// The next enumerated directory. Fails on interruption, or if every worker
    /// has exited, instead of waiting forever on the retained sender.
    pub(super) fn receive(&self) -> Result<walk::DirResult> {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(250)) {
                Ok(result) => return Ok(result),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.shared.stop.load(Ordering::Relaxed) {
                        bail!("interrupted; completed batches are saved");
                    }
                    if self.crew.iter().all(|h| h.is_finished()) {
                        bail!("scan workers stopped unexpectedly");
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("scan workers stopped unexpectedly")
                }
            }
        }
    }
}

/// Workers own shareable handles; the writer keeps the database to itself.
fn walk_loop(shared: &Shared, tx: &mpsc::SyncSender<walk::DirResult>) {
    loop {
        let job = {
            let mut q = shared.jobs.lock().unwrap();
            loop {
                // Checked before taking work so an aborted sweep does not keep
                // enumerating its backlog while the pool is being dropped.
                if shared.halt.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(job) = q.pop() {
                    break job;
                }
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                q = shared.signal.wait(q).unwrap();
            }
        };
        if tx
            .send(walk::walk_dir(&shared.stop, &shared.filter, &job))
            .is_err()
        {
            return;
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.halt.store(true, Ordering::Relaxed);
        self.shared.signal.notify_all();
        // Release the retained sender, then drain results so no worker blocks
        // on a full queue while its thread is joined.
        drop(self.tx.take());
        while self.rx.recv().is_ok() {}
        for handle in self.crew.drain(..) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
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
}
