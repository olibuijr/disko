mod password;
use super::lock;
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
    /// The hash still matches `disko`, which older installers set by default.
    pub default_password: bool,
    sessions: Mutex<HashMap<String, Session>>,
    attempts: Mutex<HashMap<IpAddr, (i64, u32)>>,
}
impl Auth {
    pub fn new(path: &Path) -> Result<Self> {
        let hash = password::load(path)?;
        let default_password = Argon2::default()
            .verify_password(b"disko", &PasswordHash::new(&hash).unwrap())
            .is_ok();
        if default_password {
            eprintln!("dashboard password is the default; set one with `sudo disko password`");
        }
        Ok(Self {
            default_password,
            hash,
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
        })
    }
    pub fn login(&self, password: &str, ip: IpAddr) -> Result<Session> {
        {
            let mut attempts = lock(&self.attempts);
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
        let mut sessions = lock(&self.sessions);
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
        let mut sessions = lock(&self.sessions);
        sessions.retain(|_, s| s.expires > now());
        sessions.get(id).cloned()
    }
    pub fn csrf(session: &Session, headers: &[Header]) -> bool {
        headers.iter().any(|h| {
            h.field.equiv("X-Disko-Token")
                && same_secret(h.value.as_str().as_bytes(), session.csrf.as_bytes())
        })
    }
    pub fn logout(&self, session: &Session) {
        lock(&self.sessions).remove(&session.id);
    }
}

/// The address a login is rate-limited by. Behind a reverse proxy every peer
/// is the proxy, so with `trust` (opt-in, DISKO_TRUST_PROXY) a loopback peer's
/// forwarded address is used instead: the last X-Forwarded-For entry, which the
/// nearest proxy appends and a client cannot forge, else X-Real-IP. Headers
/// from any other peer are ignored, since a client could set them freely.
pub fn client_ip(peer: IpAddr, headers: &[Header], trust: bool) -> IpAddr {
    if !trust || !peer.to_canonical().is_loopback() {
        return peer;
    }
    let header = |name: &'static str| {
        headers
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str())
    };
    header("X-Forwarded-For")
        .and_then(|v| v.rsplit(',').next())
        .or_else(|| header("X-Real-IP"))
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .unwrap_or(peer)
}

/// Compare a secret in time that does not depend on where the inputs differ,
/// so response timing cannot reveal a token byte by byte. Only the length,
/// which is fixed and public, can return early. Session IDs and delete-preview
/// tokens are looked up in hash maps keyed with random SipHash keys, which
/// leak no comparable prefix timing; the password goes through Argon2.
fn same_secret(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0_u8, |d, (x, y)| d | (x ^ y));
    std::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_tokens_must_match_exactly() {
        let session = Session {
            id: "s".into(),
            csrf: "a1b2c3".into(),
            expires: 0,
        };
        let header = |v: &str| vec![Header::from_bytes("X-Disko-Token", v).unwrap()];
        assert!(Auth::csrf(&session, &header("a1b2c3")));
        assert!(!Auth::csrf(&session, &header("a1b2c4")));
        assert!(!Auth::csrf(&session, &header("a1b2c")));
        assert!(!Auth::csrf(&session, &header("")));
        assert!(!Auth::csrf(&session, &[]));
        assert!(same_secret(b"", b""));
        assert!(!same_secret(b"x", b"y"));
    }

    #[test]
    fn forwarded_addresses_count_only_from_a_trusted_local_proxy() {
        let h = |pairs: &[(&str, &str)]| -> Vec<Header> {
            pairs
                .iter()
                .map(|(k, v)| Header::from_bytes(*k, *v).unwrap())
                .collect()
        };
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let lan: IpAddr = "192.168.1.50".parse().unwrap();
        let client: IpAddr = "203.0.113.7".parse().unwrap();
        let forwarded = h(&[("X-Forwarded-For", "10.9.9.9, 203.0.113.7")]);
        // Off by default, and never for a peer that is not loopback.
        assert_eq!(client_ip(local, &forwarded, false), local);
        assert_eq!(client_ip(lan, &forwarded, true), lan);
        // The proxy's own entry (the last one) wins over a forged first one.
        assert_eq!(client_ip(local, &forwarded, true), client);
        let v6: IpAddr = "::1".parse().unwrap();
        assert_eq!(client_ip(v6, &forwarded, true), client);
        let real = h(&[("X-Real-IP", "203.0.113.7")]);
        assert_eq!(client_ip(local, &real, true), client);
        // Garbage or no header: the peer.
        assert_eq!(
            client_ip(local, &h(&[("X-Forwarded-For", "junk")]), true),
            local
        );
        assert_eq!(client_ip(local, &[], true), local);
    }
}
