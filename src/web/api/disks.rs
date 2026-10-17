use super::*;
use crate::queries::filesystems;

pub(super) fn roots(c: &Connection) -> Result<Value> {
    let coverage: String = c.query_row("SELECT value FROM meta WHERE key='coverage'", [], |r| {
        r.get(0)
    })?;
    let coverage: Value = serde_json::from_str(&coverage)?;
    let mut indexes = Vec::new();
    for p in coverage["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if let Some(id) = resolve(c, Path::new(p))? {
            indexes.push(json!({"id":id,"path":p,"name":if p=="/"{"Machine index"}else{p}}));
        }
    }
    Ok(json!({"indexes":indexes,"disks":filesystems(c)?}))
}
