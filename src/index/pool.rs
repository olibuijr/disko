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
