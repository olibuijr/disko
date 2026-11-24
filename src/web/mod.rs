mod api;
mod assets;
mod auth;
mod mutations;
mod routes;
mod server;
pub use auth::set_password;
pub use server::{serve, start};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::AtomicBool},
};

#[derive(Clone)]
pub struct Options {
    pub listen: SocketAddr,
    pub password_file: PathBuf,
    /// Rate-limit logins by the forwarded client address when the peer is
    /// loopback (a reverse proxy on this host).
    pub trust_proxy: bool,
}
/// Lock web state even if a thread panicked while holding it. The guarded maps
/// and flags stay valid after any partial update, and an unwrap here would turn
/// one panic into a panic on every later request.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn password_path(db: &Path, explicit: Option<&Path>) -> PathBuf {
    explicit
        .map(Path::to_owned)
        .unwrap_or_else(|| db.parent().unwrap_or(Path::new(".")).join("web-password"))
}
struct App {
    db: PathBuf,
    auth: auth::Auth,
    rescan: Option<Arc<AtomicBool>>,
    trust_proxy: bool,
    previews: Mutex<HashMap<String, mutations::Preview>>,
    deleting: Mutex<()>,
}
struct Reply {
    status: u16,
    mime: &'static str,
    body: String,
    cookie: Option<String>,
}
impl Reply {
    fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            mime: "application/json; charset=utf-8",
            body: value.to_string(),
            cookie: None,
        }
    }
    fn asset(mime: &'static str, body: &str) -> Self {
        Self {
            status: 200,
            mime,
            body: body.into(),
            cookie: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let m = Arc::new(Mutex::new(1));
        let held = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = held.lock().unwrap();
            panic!("injected failure");
        })
        .join();
        assert!(m.is_poisoned());
        *lock(&m) += 1;
        assert_eq!(*lock(&m), 2);
    }
}
