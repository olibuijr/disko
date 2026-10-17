use super::*;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    thread::JoinHandle,
};

struct Queued {
    main: VecDeque<walk::Job>,
    tail: VecDeque<walk::Job>,
}

impl Queued {
    fn new(root: walk::Job) -> Self {
        Self {
            main: VecDeque::from([root]),
            tail: VecDeque::new(),
        }
    }
}

pub(super) struct Pool {
    jobs: Arc<Mutex<Queued>>,
    signal: Arc<Condvar>,
    halt: Arc<AtomicBool>,
    rx: mpsc::Receiver<walk::DirResult>,
    tx: Option<mpsc::SyncSender<walk::DirResult>>,
    crew: Vec<JoinHandle<()>>,
}

impl Pool {
    pub(super) fn start(
        stop: Arc<AtomicBool>,
        filter: Arc<walk::Filter>,
        count: u32,
        root: walk::Job,
    ) -> Result<Pool> {
        let threads = walk::crew_size(count);
        let (tx, rx) = mpsc::sync_channel(128);
        let jobs = Arc::new(Mutex::new(Queued::new(root)));
        let signal = Arc::new(Condvar::new());
        let halt = Arc::new(AtomicBool::new(false));
        let mut pool = Pool {
            jobs: jobs.clone(),
            signal: signal.clone(),
            halt: halt.clone(),
            rx,
            tx: Some(tx.clone()),
            crew: Vec::new(),
        };
        for _ in 0..threads {
            let (stop, filter, jobs, signal, halt, tx) = (
                stop.clone(),
                filter.clone(),
                jobs.clone(),
                signal.clone(),
                halt.clone(),
                tx.clone(),
            );
            match thread::Builder::new()
                .name("disko-walk".into())
                .spawn(move || walk_loop(stop, filter, jobs, signal, halt, tx))
            {
                Ok(handle) => pool.crew.push(handle),
                Err(e) if pool.crew.is_empty() => bail!("no scan worker: {e}"),
                Err(_) => break,
            }
        }
        Ok(pool)
    }

    pub(super) fn dispatch(&self, job: walk::Job) {
        {
            let mut q = self.jobs.lock().unwrap();
            match job.place {
                walk::Place::Front => q.main.push_front(job),
                walk::Place::Regular => q.main.push_back(job),
                walk::Place::Tail => q.tail.push_back(job),
            }
        }
        self.signal.notify_all();
    }

    pub(super) fn receive(&self) -> Option<walk::DirResult> {
        self.rx.recv().ok()
    }
}

/// Workers own shareable handles; the writer keeps the database to itself.
fn walk_loop(
    stop: Arc<AtomicBool>,
    filter: Arc<walk::Filter>,
    jobs: Arc<Mutex<Queued>>,
    signal: Arc<Condvar>,
    halt: Arc<AtomicBool>,
    tx: mpsc::SyncSender<walk::DirResult>,
) {
    loop {
        let job = {
            let mut q = jobs.lock().unwrap();
            loop {
                if let Some(job) = q.main.pop_front().or_else(|| q.tail.pop_front()) {
                    break job;
                }
                if halt.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
                    return;
                }
                q = signal.wait(q).unwrap();
            }
        };
        if tx.send(walk::walk_dir(&stop, &filter, &job)).is_err() {
            return;
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.halt.store(true, Ordering::Relaxed);
        self.signal.notify_all();
        // Release the retained sender, then drain results so no worker blocks
        // on a full queue while its thread is joined.
        drop(self.tx.take());
        while self.rx.recv().is_ok() {}
        for handle in self.crew.drain(..) {
            let _ = handle.join();
        }
    }
}
