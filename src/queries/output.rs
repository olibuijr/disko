use super::*;

pub fn rows<P: rusqlite::Params>(
    c: &Connection,
    sql: &str,
    p: P,
    apparent: bool,
    json: bool,
) -> Result<()> {
    let ids = c
        .prepare(sql)?
        .query_map(p, |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
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
fn human(n: i64) -> String {
    let mut n = n as f64;
    let mut unit = 0;
    while n >= 1024. && unit < 5 {
        n /= 1024.;
        unit += 1;
    }
    format!("{n:.1} {}", ["B", "KiB", "MiB", "GiB", "TiB", "PiB"][unit])
}
