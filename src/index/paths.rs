use super::*;

pub fn absolute(p: &Path) -> Result<PathBuf> {
    let p = if p.is_absolute() {
        p.to_owned()
    } else {
        std::env::current_dir()?.join(p)
    };
    let mut out = PathBuf::from("/");
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    Ok(out)
}

pub fn resolve(c: &Connection, p: &Path) -> Result<Option<i64>> {
    let mut id = 1;
    for part in p.components() {
        if let Component::Normal(s) = part {
            let next = c
                .prepare_cached("SELECT id FROM nodes WHERE parent=?1 AND name=?2")?
                .query_row(params![id, s.as_bytes()], |r| r.get(0))
                .optional()?;
            match next {
                Some(v) => id = v,
                None => return Ok(None),
            }
        }
    }
    Ok(Some(id))
}

pub fn path_of(c: &Connection, mut id: i64) -> Result<PathBuf> {
    let mut parts = Vec::new();
    while id > 1 {
        let (parent, name): (i64, Vec<u8>) = c
            .prepare_cached("SELECT parent,name FROM nodes WHERE id=?1")?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        parts.push(OsString::from_vec(name));
        id = parent;
    }
    let mut p = PathBuf::from("/");
    for part in parts.into_iter().rev() {
        p.push(part);
    }
    Ok(p)
}
