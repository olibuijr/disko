use super::*;
use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use serde_json::{Value, json};
use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr},
    sync::atomic::Ordering,
};
use tiny_http::{Method, Request};

pub(super) fn dispatch(app: &App, request: &mut Request, port: u16) -> Result<Reply> {
    let host = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Host"))
        .map(|h| h.value.as_str())
        .unwrap_or("");
    let valid_host = host == format!("localhost:{port}")
        || host.parse::<SocketAddr>().is_ok_and(|a| {
            a.port() == port
                && match a.ip() {
                    IpAddr::V4(ip) => {
                        ip.is_private()
                            || ip.is_loopback()
                            || ip.is_link_local()
                            || ip.is_unspecified()
                    }
                    IpAddr::V6(ip) => {
                        ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
                    }
                }
        });
    let origin = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str());
    if !valid_host || origin.is_some_and(|o| o != format!("http://{host}")) {
        return Ok(Reply::json(
            403,
            json!({"error":"same-origin LAN requests only"}),
        ));
    }
    if request.url().len() > 8192 {
        bail!("request URL is too long");
    }
    let url = request.url().to_owned();
    let (route, query) = url.split_once('?').unwrap_or((&url, ""));
    if request.method() == &Method::Get
        && let Some(asset) = assets::get(route)
    {
        return Ok(asset);
    }
    let session = app.auth.session(request.headers());
    if route == "/api/session" && request.method() == &Method::Get {
        return Ok(Reply::json(
            200,
            match session {
                Some(s) => {
                    json!({"authenticated":true,"token":s.csrf,"can_rescan":app.rescan.is_some(),"version":env!("CARGO_PKG_VERSION")})
                }
                None => json!({"authenticated":false}),
            },
        ));
    }
    if route == "/api/login" && request.method() == &Method::Post {
        let ip = request
            .remote_addr()
            .map(|a| a.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let body = body(request)?;
        return Ok(
            match app.auth.login(body["password"].as_str().unwrap_or(""), ip) {
                Ok(s) => {
                    let mut r = Reply::json(
                        200,
                        json!({"authenticated":true,"token":s.csrf,"can_rescan":app.rescan.is_some(),"version":env!("CARGO_PKG_VERSION")}),
                    );
                    r.cookie = Some(format!(
                        "disko_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200",
                        s.id
                    ));
                    r
                }
                Err(e) => Reply::json(401, json!({"error":e.to_string()})),
            },
        );
    }
    let Some(session) = session else {
        return Ok(Reply::json(
            401,
            json!({"error":"sign in to view this index"}),
        ));
    };
    if request.method() == &Method::Post {
        if !auth::Auth::csrf(&session, request.headers()) {
            return Ok(Reply::json(403, json!({"error":"invalid session token"})));
        }
        match route {
            "/api/logout" => {
                app.auth.logout(&session);
                let mut r = Reply::json(200, json!({"ok":true}));
                r.cookie =
                    Some("disko_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0".into());
                return Ok(r);
            }
            "/api/rescan" => {
                app.rescan
                    .as_ref()
                    .context("dashboard is read-only")?
                    .store(true, Ordering::Relaxed);
                return Ok(Reply::json(202, json!({"queued":true})));
            }
            "/api/delete/preview" | "/api/delete" => {
                return mutations::handle(app, &session, route, body(request)?);
            }
            _ => return Ok(Reply::json(404, json!({"error":"not found"}))),
        }
    }
    if request.method() != &Method::Get {
        return Ok(Reply::json(405, json!({"error":"method not allowed"})));
    }
    let params: HashMap<_, _> = query
        .split('&')
        .filter_map(|s| s.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect();
    Ok(Reply::json(200, api::query(&app.db, route, &params)?))
}

fn decode(s: &str) -> String {
    percent_decode_str(&s.replace('+', " "))
        .decode_utf8_lossy()
        .into_owned()
}
fn body(request: &mut Request) -> Result<Value> {
    let mut body = String::new();
    request.as_reader().take(4097).read_to_string(&mut body)?;
    if body.len() > 4096 {
        bail!("request body is too large");
    }
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&body).context("invalid JSON body")
}
