use super::*;

pub fn largest_under(
    c: &Connection,
    root: i64,
    kind: i64,
    limit: usize,
    apparent: bool,
) -> Result<Vec<i64>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    // Best-first traversal: subtree totals are upper bounds. No filesystem reads.
    let col = if apparent { "bytes" } else { "blocks" };
    let mut queue = BinaryHeap::new();
    queue.push((i64::MAX, root, 1));
    let mut found = Vec::new();
    let mut dirs = c.prepare(&format!(
        "SELECT id,{col} FROM nodes WHERE parent=?1 AND kind=1 ORDER BY {col} DESC"
    ))?;
    let mut files = c.prepare(&format!(
        "SELECT id,{col} FROM nodes WHERE parent=?1 AND kind=0 ORDER BY {col} DESC LIMIT ?2"
    ))?;
    while let Some((score, id, k)) = queue.pop() {
        if k == kind && id != root {
            found.push((score, id));
            if found.len() >= limit {
                break;
            }
        }
        if k == 1 {
            for row in
                dirs.query_map([id], |r| Ok((r.get::<_, i64>(1)?, r.get::<_, i64>(0)?, 1)))?
            {
                queue.push(row?);
            }
            if kind == 0 {
                for row in files.query_map(params![id, limit as i64], |r| {
                    Ok((r.get::<_, i64>(1)?, r.get::<_, i64>(0)?, 0))
                })? {
                    queue.push(row?);
                }
            }
        }
    }
    Ok(found.into_iter().map(|(_, id)| id).collect())
}
