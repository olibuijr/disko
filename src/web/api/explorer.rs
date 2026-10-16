use super::*;
pub(super) fn query(
    c: &Connection,
    route: &str,
    params: &HashMap<String, String>,
) -> Result<Value> {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(100)
        .clamp(1, 500);
    let apparent = params.get("metric").is_some_and(|v| v == "apparent");
    let col = if apparent { "bytes" } else { "blocks" };
    Ok(match route {
        "/api/tree" | "/api/info" => {
            let id = if let Some(id) = params.get("id") {
                id.parse::<i64>()?
            } else {
                required(
                    c,
                    &absolute(Path::new(
                        params.get("path").map(String::as_str).unwrap_or("/"),
                    ))?,
                )?
            };
            let info = node(c, id)?;
            if route == "/api/info" {
                info
            } else {
                let mut ids = Vec::new();
                // A bounded merge of per-kind indexed lists also handles huge flat directories.
                for kind in 0..4 {
                    ids.extend(c.prepare(&format!("SELECT id,{col} FROM nodes WHERE parent=?1 AND kind=?2 ORDER BY {col} DESC LIMIT ?3"))?.query_map(params![id,kind,limit],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?);
                }
                ids.sort_by_key(|a| std::cmp::Reverse(a.1));
                ids.truncate(limit as usize);
                let children = ids
                    .iter()
                    .map(|(id, _)| node(c, *id))
                    .collect::<Result<Vec<_>>>()?;
                let mut ancestors = Vec::new();
                let mut at = id;
                while at > 0 {
                    ancestors.push(node(c, at)?);
                    at = c.query_row("SELECT parent FROM nodes WHERE id=?1", [at], |r| r.get(0))?;
                }
                ancestors.reverse();
                json!({"node":info,"children":children,"ancestors":ancestors})
            }
        }
        "/api/largest" => {
            let kind = if params.get("kind").is_some_and(|v| v == "dirs") {
                1
            } else {
                0
            };
            let id = params
                .get("id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(1);
            let ids = if id == 1 {
                c.prepare(&format!(
                    "SELECT id FROM nodes WHERE kind={kind} AND id!=1 ORDER BY {col} DESC LIMIT ?1"
                ))?
                .query_map([limit], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
            } else {
                largest_under(c, id, kind, limit as usize, apparent)?
            };
            json!(
                ids.iter()
                    .map(|id| node(c, *id))
                    .collect::<Result<Vec<_>>>()?
            )
        }
        "/api/find" => {
            let text = params.get("q").context("missing search prefix")?;
            if text.is_empty() || text.len() > 256 {
                bail!("enter a filename prefix of 1–256 bytes");
            }
            let lo = text.as_bytes();
            let mut hi = lo.to_vec();
            *hi.last_mut().unwrap() += 1;
            let ids = c
                .prepare("SELECT id FROM nodes WHERE name>=?1 AND name<?2 ORDER BY name LIMIT ?3")?
                .query_map(params![lo, hi, limit], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            json!(
                ids.iter()
                    .map(|id| node(c, *id))
                    .collect::<Result<Vec<_>>>()?
            )
        }
        _ => unreachable!(),
    })
}
