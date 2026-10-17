use super::*;

/// Kind code, apparent bytes and allocated bytes, as stored in `nodes`.
pub(super) fn shape(m: &fs::Metadata) -> (i64, i64, i64) {
    let kind = if m.is_dir() {
        1
    } else if m.is_file() {
        0
    } else if m.file_type().is_symlink() {
        2
    } else {
        3
    };
    let bytes = if kind == 1 || kind == 3 {
        0
    } else {
        m.len() as i64
    };
    (kind, bytes, (m.blocks() * 512) as i64)
}

/// An indexed child as last recorded, for change detection during scans.
pub(super) struct Known {
    pub(super) id: i64,
    fields: [i64; 11],
}

impl Known {
    /// True when nothing Disko records has changed, so the row needs no write.
    pub(super) fn matches(&self, m: &fs::Metadata) -> bool {
        let (kind, bytes, blocks) = shape(m);
        self.fields
            == [
                kind,
                bytes,
                blocks,
                m.mtime(),
                m.ctime(),
                m.dev() as i64,
                m.ino() as i64,
                m.nlink() as i64,
                i64::from(m.mode()),
                i64::from(m.uid()),
                i64::from(m.gid()),
            ]
    }
}

impl Index {
    /// Every indexed child of a directory, keyed by name.
    pub(super) fn children_of(&self, parent: i64) -> Result<HashMap<Vec<u8>, Known>> {
        let mut out = HashMap::new();
        let mut stmt = self.c.prepare_cached(
            "SELECT id,name,kind,size,allocated,mtime,ctime,dev,ino,links,mode,uid,gid FROM nodes WHERE parent=?1",
        )?;
        let mut rows = stmt.query([parent])?;
        while let Some(r) = rows.next()? {
            let mut fields = [0_i64; 11];
            for (i, f) in fields.iter_mut().enumerate() {
                *f = r.get(i + 2)?;
            }
            out.insert(
                r.get::<_, Vec<u8>>(1)?,
                Known {
                    id: r.get(0)?,
                    fields,
                },
            );
        }
        Ok(out)
    }
}
