use super::*;

pub(super) fn query(c: &Connection, route: &str) -> Result<Value> {
    if route == "/api/activity" {
        let mut statement =
            c.prepare("SELECT ts,path,kind FROM activity ORDER BY id DESC LIMIT 60")?;
        let rows = statement
            .query_map([], |row| {
                Ok(json!({
                    "ts": row.get::<_,i64>(0)?,
                    "path": String::from_utf8_lossy(&row.get::<_,Vec<u8>>(1)?),
                    "kind": row.get::<_,String>(2)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        return Ok(json!(rows));
    }
    let mut scans = c.prepare(
        "SELECT id,started,finished,visited,errors,watch_errors,reason,state
         FROM scans ORDER BY id DESC LIMIT 100",
    )?;
    let scans = scans
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_,i64>(0)?,
                "started": row.get::<_,i64>(1)?,
                "finished": row.get::<_,Option<i64>>(2)?,
                "visited": row.get::<_,i64>(3)?,
                "errors": row.get::<_,i64>(4)?,
                "watch_errors": row.get::<_,i64>(5)?,
                "reason": row.get::<_,String>(6)?,
                "state": row.get::<_,String>(7)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut samples = c.prepare(
        "SELECT ts,entries,files,bytes,allocated,visited,watches,state
         FROM samples ORDER BY ts DESC LIMIT 720",
    )?;
    let samples = samples
        .query_map([], |row| {
            Ok(json!({
                "ts": row.get::<_,i64>(0)?,
                "entries": row.get::<_,i64>(1)?,
                "files": row.get::<_,i64>(2)?,
                "bytes": row.get::<_,i64>(3)?,
                "allocated": row.get::<_,i64>(4)?,
                "visited": row.get::<_,i64>(5)?,
                "watches": row.get::<_,i64>(6)?,
                "state": row.get::<_,String>(7)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"scans":scans,"samples":samples}))
}
