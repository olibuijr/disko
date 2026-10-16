use super::*;
use anyhow::Result;
use std::{sync::atomic::Ordering, time::Duration};
use tiny_http::{Header, Server};

pub fn start(
    db: PathBuf,
    options: Options,
    stop: Arc<AtomicBool>,
    rescan: Option<Arc<AtomicBool>>,
) -> Result<()> {
    let auth = auth::Auth::new(&options.password_file)?;
    let server = Arc::new(
        Server::http(options.listen)
            .map_err(|e| anyhow::anyhow!("dashboard {}: {e}", options.listen))?,
    );
    let port = server.server_addr().to_ip().unwrap().port();
    let app = Arc::new(App {
        db,
        auth,
        rescan,
        previews: Mutex::new(HashMap::new()),
        deleting: Mutex::new(()),
    });
    eprintln!("Disko dashboard: http://{}:{port}", options.listen.ip());
    for _ in 0..4 {
        let (server, app, stop) = (server.clone(), app.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let mut request = match server.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(r)) => r,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                let reply = match routes::dispatch(&app, &mut request, port) {
                    Ok(reply) => reply,
                    Err(e) => Reply::json(400, serde_json::json!({"error":e.to_string()})),
                };
                let mut response =
                    tiny_http::Response::from_string(reply.body).with_status_code(reply.status);
                for (key, value) in [
                    ("Content-Type", reply.mime),
                    ("Cache-Control", "no-store"),
                    ("X-Content-Type-Options", "nosniff"),
                    ("Referrer-Policy", "no-referrer"),
                    (
                        "Content-Security-Policy",
                        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
                    ),
                ] {
                    response.add_header(Header::from_bytes(key, value).unwrap());
                }
                if let Some(cookie) = reply.cookie {
                    response.add_header(Header::from_bytes("Set-Cookie", cookie).unwrap());
                }
                let _ = request.respond(response);
            }
        });
    }
    Ok(())
}

pub fn serve(
    db: PathBuf,
    options: Options,
    stop: Arc<AtomicBool>,
    rescan: Option<Arc<AtomicBool>>,
) -> Result<()> {
    start(db, options, stop.clone(), rescan)?;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}
