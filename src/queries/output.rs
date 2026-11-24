use super::*;

/// The first `limit` IDs from `sql` that are still in the tree. Removed
/// subtrees stay in the table until garbage collection deletes them in batches,
/// so global queries skip them here; `sql` must not apply the limit itself.
pub fn live_ids<P: rusqlite::Params>(
    c: &Connection,
    sql: &str,
    p: P,
    limit: usize,
) -> Result<Vec<i64>> {
    let mut stmt = c.prepare(sql)?;
    let mut rows = stmt.query(p)?;
    let mut ids = Vec::new();
    while ids.len() < limit
        && let Some(r) = rows.next()?
    {
        let id: i64 = r.get(0)?;
        if is_live(c, id)? {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub fn rows<P: rusqlite::Params>(
    c: &Connection,
    sql: &str,
    p: P,
    limit: usize,
    apparent: bool,
    json: bool,
) -> Result<()> {
    let ids = live_ids(c, sql, p, limit)?;
    print_rows(c, &ids, apparent, json)
}
pub fn print_rows(c: &Connection, ids: &[i64], apparent: bool, json_output: bool) -> Result<()> {
    let rows = ids
        .iter()
        .map(|id| node(c, *id))
        .collect::<Result<Vec<_>>>()?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        let mut out = io::BufWriter::new(io::stdout().lock());
        writeln!(
            out,
            "{:>11} {:>10}  PATH",
            if apparent { "APPARENT" } else { "ALLOCATED" },
            "FILES"
        )?;
        for v in rows {
            writeln!(
                out,
                "{:>11} {:>10}  {}",
                human(
                    v[if apparent {
                        "apparent_bytes"
                    } else {
                        "allocated_bytes"
                    }]
                    .as_i64()
                    .unwrap()
                ),
                v["files"],
                v["path"].as_str().unwrap().escape_debug()
            )?;
        }
    }
    Ok(())
}
pub(super) fn human(n: i64) -> String {
    let mut n = n as f64;
    let mut unit = 0;
    while n >= 1024. && unit < 5 {
        n /= 1024.;
        unit += 1;
    }
    format!("{n:.1} {}", ["B", "KiB", "MiB", "GiB", "TiB", "PiB"][unit])
}
