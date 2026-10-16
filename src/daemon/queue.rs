use super::*;
use std::sync::Mutex;

pub(super) struct EventQueue(Arc<Mutex<HashMap<PathBuf, bool>>>);
impl EventQueue {
    pub fn start(
        rx: mpsc::Receiver<notify::Result<Event>>,
        roots: Vec<PathBuf>,
        excludes: Vec<PathBuf>,
        overflow: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let output = pending.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => {
                        let mut buffer = output.lock().unwrap();
                        // Repeated writes to a hot file occupy one slot during long scans.
                        events::collect(event, &roots, &excludes, &mut buffer, &overflow);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self(pending)
    }
    pub fn take(&self) -> HashMap<PathBuf, bool> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}
