use super::*;

const SCHEMA_NOTES: &str = include_str!("schema.md");

/// One verdict for scripts and agents: ok, warn or fail, with reasons and the
/// command to look further. Exit status: 0 ok, 2 warn, 1 fail.
pub(super) fn health(c: &Connection, db: &Path, path: Option<&Path>, as_json: bool) -> Result<i32> {
    let s = status_value(c, db)?;
    let state = s["state"].as_str().unwrap_or("unknown").to_string();
    let age = s["heartbeat_age_seconds"].as_i64().unwrap_or(i64::MAX);
    let alive = s["writer_alive"].as_bool().unwrap_or(false);
    let (mut fail, mut warn) = (Vec::<String>::new(), Vec::<String>::new());
    if !alive {
        fail.push(
            "the daemon is not running, so totals no longer change (systemctl status disko)".into(),
        );
    } else if age > 900 {
        fail.push(format!(
            "no heartbeat for {age}s; the writer may be stuck (journalctl -u disko)"
        ));
    } else if age > 120 {
        warn.push(format!("last heartbeat {age}s ago; the writer is busy"));
    }
    if state == "scanning" {
        warn.push(
            "a scan is running; totals of folders it has not reached yet may be stale or partial"
                .into(),
        );
    }
    let last_scan = s["last_scan"].as_i64().unwrap_or(0);
    if last_scan == 0 {
        warn.push("no scan has completed yet".into());
    }
    let errors = s["metadata_errors"].as_i64().unwrap_or(0);
    if errors >= 50 {
        warn.push(format!(
            "{errors} entries could not be read since the daemon started"
        ));
    }
    let watch_errors = s["watch_errors"].as_i64().unwrap_or(0);
    if watch_errors > 0 {
        warn.push(format!(
            "{watch_errors} folders could not be watched; their changes wait for the next scan"
        ));
    }
    let mut indexed = Value::Null;
    if let Some(p) = path {
        let p = absolute(p)?;
        let found = resolve(c, &p)?.is_some();
        indexed = json!(found);
        if !found {
            fail.push(format!("{} is not in the index", p.display()));
        }
    }
    let level = if !fail.is_empty() {
        "fail"
    } else if !warn.is_empty() {
        "warn"
    } else {
        "ok"
    };
    let v = json!({
        "level": level, "state": state, "writer_alive": alive, "heartbeat_age_seconds": age,
        "last_scan": last_scan, "scan_mode": s["scan_mode"], "watch_backend": s["watch_backend"],
        "entries": s["totals"]["entries"], "path_indexed": indexed, "problems": fail, "warnings": warn,
    });
    if as_json {
        println!("{v}");
    } else {
        println!("{level}: {state}, heartbeat {age}s ago");
        for line in fail.iter().chain(&warn) {
            println!("  - {line}");
        }
    }
    Ok(match level {
        "ok" => 0,
        "warn" => 2,
        _ => 1,
    })
}

/// Ask the running daemon for a deep scan (SIGUSR1, the same as the dashboard's button).
pub(super) fn rescan(c: &Connection, as_json: bool) -> Result<()> {
    let pid: i32 = c
        .query_row("SELECT value FROM meta WHERE key='pid'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if pid <= 0 || !Path::new(&format!("/proc/{pid}")).exists() {
        bail!("the daemon is not running; start it with: sudo systemctl start disko");
    }
    // SAFETY: kill only sends a signal to the recorded daemon process.
    if unsafe { libc::kill(pid, libc::SIGUSR1) } != 0 {
        let e = io::Error::last_os_error();
        bail!("could not signal the daemon (pid {pid}): {e}; run it as root: sudo disko rescan");
    }
    if as_json {
        println!("{}", json!({"requested": "deep scan", "pid": pid}));
    } else {
        println!("asked the daemon (pid {pid}) for a deep scan; follow it with: disko health");
    }
    Ok(())
}

/// The index schema: the stored CREATE statements and what the columns mean.
pub(super) fn schema(c: &Connection, as_json: bool) -> Result<()> {
    let mut s = c.prepare(
        "SELECT name,sql FROM sqlite_master WHERE type='table' AND sql IS NOT NULL ORDER BY name",
    )?;
    let tables = s
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if as_json {
        let t: Vec<_> = tables
            .iter()
            .map(|(n, q)| json!({"table": n, "sql": q}))
            .collect();
        println!("{}", json!({"tables": t, "notes": SCHEMA_NOTES}));
    } else {
        for (_, q) in &tables {
            println!("{q};");
        }
        println!("\n{SCHEMA_NOTES}");
    }
    Ok(())
}
