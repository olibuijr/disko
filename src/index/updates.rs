use super::*;

impl Index {
    pub(super) fn upsert(
        &mut self,
        parent: i64,
        name: &OsStr,
        m: &fs::Metadata,
        seen: i64,
    ) -> Result<i64> {
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
        let blocks = (m.blocks() * 512) as i64;
        let old: Option<(i64, i64, i64, i64)> = self
            .c
            .prepare_cached("SELECT id,kind,size,allocated FROM nodes WHERE parent=?1 AND name=?2")?
            .query_row(params![parent, name.as_bytes()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let old = if let Some((id, k, _, _)) = old {
            if k != kind {
                self.remove(id)?;
                None
            } else {
                old
            }
        } else {
            None
        };
        let d = match old {
            Some((_, _, s, b)) => Delta {
                bytes: bytes - s,
                blocks: blocks - b,
                files: 0,
                entries: 0,
            },
            None => Delta {
                bytes,
                blocks,
                files: i64::from(kind == 0),
                entries: 1,
            },
        };
        let id=self.c.prepare_cached("INSERT INTO nodes(parent,name,kind,size,allocated,bytes,blocks,files,entries,mtime,ctime,dev,ino,links,mode,uid,gid,seen)
          VALUES(?1,?2,?3,?4,?5,?4,?5,?6,1,?7,?8,?9,?10,?11,?12,?13,?14,?15)
          ON CONFLICT(parent,name) DO UPDATE SET size=excluded.size,allocated=excluded.allocated,
          bytes=nodes.bytes+excluded.size-nodes.size,blocks=nodes.blocks+excluded.allocated-nodes.allocated,
          mtime=excluded.mtime,ctime=excluded.ctime,dev=excluded.dev,ino=excluded.ino,links=excluded.links,
          mode=excluded.mode,uid=excluded.uid,gid=excluded.gid,seen=excluded.seen RETURNING id")?.query_row(params![parent,name.as_bytes(),kind,bytes,blocks,i64::from(kind==0),m.mtime(),m.ctime(),m.dev() as i64,m.ino() as i64,m.nlink() as i64,m.mode(),m.uid(),m.gid(),seen],|r|r.get(0))?;
        if kind == 1 {
            self.parents.insert(id, parent);
        }
        self.ancestors(parent, d)?;
        self.dirty += 1;
        self.visited += 1;
        self.maybe_commit()?;
        Ok(id)
    }

    pub(super) fn remove(&mut self, id: i64) -> Result<()> {
        // Pending descendant deltas must be applied before reading subtree totals.
        let pending: Vec<_> = self.deltas.drain().collect();
        for (i, d) in pending {
            self.c.prepare_cached("UPDATE nodes SET bytes=bytes+?2,blocks=blocks+?3,files=files+?4,entries=entries+?5 WHERE id=?1")?.execute(params![i,d.bytes,d.blocks,d.files,d.entries])?;
        }
        let row: Option<(i64, Delta, i64)> = self
            .c
            .prepare_cached("SELECT parent,bytes,blocks,files,entries,kind FROM nodes WHERE id=?1")?
            .query_row([id], |r| {
                Ok((
                    r.get(0)?,
                    Delta {
                        bytes: -r.get::<_, i64>(1)?,
                        blocks: -r.get::<_, i64>(2)?,
                        files: -r.get::<_, i64>(3)?,
                        entries: -r.get::<_, i64>(4)?,
                    },
                    r.get(5)?,
                ))
            })
            .optional()?;
        if let Some((parent, d, kind)) = row {
            if id == 1 {
                bail!("refusing to remove the index root");
            }
            if kind == 1 && !self.watched.is_empty() {
                let path = path_of(&self.c, id)?;
                self.forget_watches(&path);
            }
            if kind == 1 {
                // Drop exactly the cached parents of removed directories; row IDs
                // can be reused, so stale entries would misroute later deltas.
                let dirs = self.c.prepare_cached("WITH RECURSIVE subtree(id,kind) AS (SELECT id,kind FROM nodes WHERE id=?1 UNION ALL SELECT n.id,n.kind FROM nodes n JOIN subtree s ON n.parent=s.id WHERE s.kind=1) SELECT id FROM subtree WHERE kind=1")?.query_map([id], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                for dir in dirs {
                    self.parents.remove(&dir);
                }
            }
            self.c.prepare_cached("WITH RECURSIVE subtree(id) AS (SELECT ?1 UNION ALL SELECT n.id FROM nodes n JOIN subtree s ON n.parent=s.id) DELETE FROM nodes WHERE id IN (SELECT id FROM subtree)")?.execute([id])?;
            self.ancestors(parent, d)?;
            self.dirty += 1;
        }
        Ok(())
    }

    /// Drop a path and everything below it from the index, if present.
    pub fn forget_path(&mut self, p: &Path) -> Result<()> {
        match resolve(&self.c, p)? {
            Some(id) if id != 1 => self.remove(id),
            _ => Ok(()),
        }
    }

    /// Remove every entry below a directory but keep the directory itself.
    pub fn prune_children(&mut self, id: i64) -> Result<()> {
        let children = self
            .c
            .prepare_cached("SELECT id FROM nodes WHERE parent=?1")?
            .query_map([id], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for child in children {
            self.remove(child)?;
        }
        Ok(())
    }

    pub(super) fn ensure(&mut self, p: &Path) -> Result<Option<i64>> {
        if p == Path::new("/") {
            return Ok(Some(1));
        }
        let parent = p.parent().context("path has no parent")?;
        let parent_id = match resolve(&self.c, parent)? {
            Some(id) => id,
            None => match self.ensure(parent)? {
                Some(id) => id,
                None => return Ok(None),
            },
        };
        match fs::symlink_metadata(p) {
            Ok(m) => self
                .upsert(parent_id, p.file_name().context("missing filename")?, &m, 0)
                .map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => {
                self.error(p, &e);
                Ok(None)
            }
        }
    }

    pub fn update_hardlinks(&mut self, p: &Path) -> Result<()> {
        let Ok(m) = fs::symlink_metadata(p) else {
            return Ok(());
        };
        if !m.is_file() || m.nlink() < 2 {
            return Ok(());
        }
        let ids = self
            .c
            .prepare_cached("SELECT id FROM nodes WHERE dev=?1 AND ino=?2 AND links>1 AND kind=0")?
            .query_map(params![m.dev() as i64, m.ino() as i64], |r| {
                r.get::<_, i64>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            let path = path_of(&self.c, id)?;
            if path != p {
                self.ensure(&path)?;
            }
        }
        Ok(())
    }
}
