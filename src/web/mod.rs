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
    sync::{Arc, Mutex, atomic::AtomicBool},
};

#[derive(Clone)]
pub struct Options {
    pub listen: SocketAddr,
    pub password_file: PathBuf,
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
