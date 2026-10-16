use super::*;
pub(super) fn sql(c: &Connection, query: &str) -> Result<()> {
    let mut s = c.prepare(query)?;
    if !s.readonly() {
        bail!("only read-only SQL is allowed");
    }
    let names = s
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let mut rs = s.query([])?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    while let Some(r) = rs.next()? {
        let mut obj = serde_json::Map::new();
        for (i, name) in names.iter().enumerate() {
            let v = match r.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(v) => json!(v),
                rusqlite::types::ValueRef::Real(v) => json!(v),
                rusqlite::types::ValueRef::Text(v) | rusqlite::types::ValueRef::Blob(v) => {
                    json!(String::from_utf8_lossy(v))
                }
            };
            obj.insert(name.clone(), v);
        }
        writeln!(out, "{}", Value::Object(obj))?;
    }
    Ok(())
}
