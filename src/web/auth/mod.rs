mod password;
use crate::index::now;
use anyhow::{Result, bail};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
pub use password::{random_token, set_password};
use std::{collections::HashMap, net::IpAddr, path::Path, sync::Mutex};
use tiny_http::Header;

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub csrf: String,
    pub expires: i64,
}
pub struct Auth {
    hash: String,
    sessions: Mutex<HashMap<String, Session>>,
    attempts: Mutex<HashMap<IpAddr, (i64, u32)>>,
}
impl Auth {
    pub fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            hash: password::load(path)?,
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
        })
    }
    pub fn login(&self, password: &str, ip: IpAddr) -> Result<Session> {
        {
            let mut attempts = self.attempts.lock().unwrap();
            attempts.retain(|_, (start, _)| now() - *start < 60);
            if attempts.len() >= 1024 && !attempts.contains_key(&ip) {
                bail!("login rate limit; try again in a minute");
            }
            let entry = attempts.entry(ip).or_insert((now(), 0));
            if entry.1 >= 5 {
                bail!("login rate limit; try again in a minute");
            }
            entry.1 += 1;
        }
        if password.len() > 1024
            || Argon2::default()
                .verify_password(password.as_bytes(), &PasswordHash::new(&self.hash).unwrap())
                .is_err()
        {
            bail!("Incorrect password");
        }
        let session = Session {
            id: random_token()?,
            csrf: random_token()?,
            expires: now() + 12 * 3600,
        };
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| s.expires > now());
        if sessions.len() >= 64 {
            bail!("too many active sessions");
        }
        sessions.insert(session.id.clone(), session.clone());
        Ok(session)
    }
    pub fn session(&self, headers: &[Header]) -> Option<Session> {
        let cookie = headers
            .iter()
            .find(|h| h.field.equiv("Cookie"))?
            .value
            .as_str();
        let id = cookie
            .split(';')
            .filter_map(|c| c.trim().split_once('='))
            .find(|(k, _)| *k == "disko_session")?
            .1;
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, s| s.expires > now());
        sessions.get(id).cloned()
    }
    pub fn csrf(session: &Session, headers: &[Header]) -> bool {
        headers
            .iter()
            .any(|h| h.field.equiv("X-Disko-Token") && h.value.as_str() == session.csrf)
    }
    pub fn logout(&self, session: &Session) {
        self.sessions.lock().unwrap().remove(&session.id);
    }
}
