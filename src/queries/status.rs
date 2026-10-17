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
