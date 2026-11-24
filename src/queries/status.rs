use super::*;

pub fn status_value(c: &Connection, db: &Path) -> Result<Value> {
    let mut meta = serde_json::Map::new();
    for row in c
        .prepare("SELECT key,value FROM meta")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
    {
        let (k, v) = row?;
        meta.insert(k, serde_json::from_str(&v).unwrap_or(Value::String(v)));
    }
    let heartbeat = meta.get("heartbeat").and_then(Value::as_i64).unwrap_or(0);
    meta.insert("heartbeat_age_seconds".into(), json!(now() - heartbeat));
    let pid = meta.get("pid").and_then(Value::as_u64).unwrap_or(0);
    meta.insert(
        "writer_alive".into(),
        json!(pid > 0 && PathBuf::from(format!("/proc/{pid}")).exists()),
    );
    meta.insert("last_complete_scan".into(), last_complete_scan(c)?);
    meta.insert("totals".into(), node(c, 1)?);
    meta.insert("database_bytes".into(), json!(std::fs::metadata(db)?.len()));
    if let Some(roots) = meta
        .get("coverage")
        .and_then(|v| v.get("roots"))
        .and_then(Value::as_array)
    {
        let capacities:Vec<_>=roots.iter().filter_map(Value::as_str).map(|p|json!({"root":p,"capacity_bytes":fs2::total_space(p).ok(),"available_bytes":fs2::available_space(p).ok()})).collect();
        meta.insert("filesystems".into(), json!(capacities));
    }
    meta.insert("disks".into(), json!(filesystems(c)?));
    Ok(Value::Object(meta))
}

/// The newest finished scan with its own error counts. `metadata_errors` and
/// `watch_errors` in `meta` keep counting from daemon start (scans and live
/// events), so they say nothing about whether the latest scan was clean.
fn last_complete_scan(c: &Connection) -> Result<Value> {
    let row = c
        .query_row(
            "SELECT started,finished,visited,errors,watch_errors,reason FROM scans WHERE state='complete' ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    Ok(match row {
        Some((started, finished, visited, errors, watch_errors, reason)) => {
            let mode = ["deep", "incremental"]
                .into_iter()
                .find(|m| reason.ends_with(&format!("({m})")));
            json!({
                "started": started, "finished": finished, "seconds": finished - started,
                "mode": mode, "reason": reason, "entries": visited,
                "metadata_errors": errors, "watch_errors": watch_errors,
            })
        }
        None => Value::Null,
    })
}
