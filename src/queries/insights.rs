use super::*;

/// Folders that tools recreate on demand, by name. Directories holding a
/// `CACHEDIR.TAG` (Cargo `target/`, many caches) are found the same way.
const REBUILDABLE: &[(&str, &str)] = &[
    ("node_modules", "Node packages"),
    (".venv", "Python environment"),
    ("venv", "Python environment"),
    ("__pycache__", "Python bytecode"),
    (".pytest_cache", "Test cache"),
    (".mypy_cache", "Type-check cache"),
    (".tox", "Python test environments"),
    (".gradle", "Gradle cache"),
    (".next", "Next.js build"),
    (".nuxt", "Nuxt build"),
    (".turbo", "Turborepo cache"),
    (".parcel-cache", "Parcel cache"),
    (".svelte-kit", "SvelteKit build"),
    (".angular", "Angular cache"),
];

/// The largest rebuildable folders, outermost first when they nest.
pub fn cleanup(c: &Connection, limit: usize, min_bytes: i64) -> Result<Vec<Value>> {
    let mut candidates: Vec<(i64, i64, &str)> = Vec::new();
    {
        let mut tagged = c.prepare(
            "SELECT p.id,p.blocks FROM nodes t JOIN nodes p ON p.id=t.parent
             WHERE t.name=?1 AND t.kind=0 AND p.blocks>=?2",
        )?;
        for row in tagged.query_map(params![b"CACHEDIR.TAG".as_slice(), min_bytes], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })? {
            let (id, blocks) = row?;
            candidates.push((id, blocks, "Build cache"));
        }
        let mut named =
            c.prepare("SELECT id,blocks FROM nodes WHERE name=?1 AND kind=1 AND blocks>=?2")?;
        for (name, label) in REBUILDABLE {
            for row in named.query_map(params![name.as_bytes(), min_bytes], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })? {
                let (id, blocks) = row?;
                candidates.push((id, blocks, label));
            }
        }
    }
    candidates.sort_by_key(|(id, blocks, _)| (std::cmp::Reverse(*blocks), *id));
    candidates.dedup_by_key(|(id, _, _)| *id);
    // Bigger folders come first, so a nested candidate is dropped once an
    // ancestor that already contains it has been kept.
    let mut kept: Vec<(PathBuf, Value)> = Vec::new();
    for (id, _, label) in candidates {
        if kept.len() >= limit {
            break;
        }
        let path = path_of(c, id)?;
        if kept.iter().any(|(p, _)| path.starts_with(p)) {
            continue;
        }
        let mut v = node(c, id)?;
        v["reason"] = json!(label);
        kept.push((path, v));
    }
    Ok(kept.into_iter().map(|(_, v)| v).collect())
}

/// Large folders that grew most since about `days` ago, preferring the
/// deepest folder that explains most of a parent's growth.
pub fn growth(c: &Connection, days: i64, limit: usize) -> Result<Value> {
    let today = now() / 86400;
    let base: Option<i64> = c
        .query_row(
            "SELECT coalesce((SELECT max(day) FROM growth WHERE day<=?1),(SELECT min(day) FROM growth))",
            [today - days],
            |r| r.get(0),
        )
        .unwrap_or(None);
    let Some(base) = base.filter(|b| *b < today) else {
        return Ok(json!({"since": null, "folders": []}));
    };
    let mut rows = c.prepare(
        "SELECT n.id,n.blocks-coalesce(g.blocks,0) FROM nodes n LEFT JOIN growth g ON g.id=n.id AND g.day=?1
         WHERE n.kind=1 AND n.blocks>=?2 AND n.blocks-coalesce(g.blocks,0)>=?3",
    )?;
    let mut grew: Vec<(PathBuf, i64, i64)> = Vec::new();
    for row in rows.query_map(params![base, 1_i64 << 30, 256_i64 << 20], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (id, delta) = row?;
        grew.push((path_of(c, id)?, id, delta));
    }
    // Skip a folder when one of its subfolders accounts for most of its growth.
    let explained = |p: &Path, delta: i64| {
        grew.iter()
            .any(|(q, _, d)| q != p && q.starts_with(p) && *d * 4 >= delta * 3)
    };
    let mut picked: Vec<(i64, i64)> = grew
        .iter()
        .filter(|(p, _, d)| !explained(p, *d))
        .map(|(_, id, d)| (*id, *d))
        .collect();
    picked.sort_by_key(|(_, d)| std::cmp::Reverse(*d));
    picked.truncate(limit);
    let folders = picked
        .into_iter()
        .map(|(id, delta)| {
            let mut v = node(c, id)?;
            v["grew_bytes"] = json!(delta);
            Ok(v)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"since": base * 86400, "folders": folders}))
}
