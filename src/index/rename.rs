use super::*;

impl Index {
    /// Apply a rename that the kernel reported as one event (fanotify
    /// `FAN_RENAME`, or a paired inotify move): the indexed entry and its whole
    /// subtree move by updating one row, instead of being deleted and rescanned.
    /// Returns true when the moved entry is a directory.
    pub fn rename(&mut self, old: &Path, new: &Path) -> Result<bool> {
        let movable = self.relevant(old)
            && self.relevant(new)
            && !self.skip.contains(old)
            && !self.skip.contains(new)
            && self.watched_under(old).is_empty();
        let id = if movable {
            resolve(&self.c, old)?
        } else {
            None
        };
        let parent = match (id, new.parent(), new.file_name()) {
            (Some(_), Some(p), Some(_)) if p != old && !p.starts_with(old) => self.ensure(p)?,
            _ => None,
        };
        let (Some(id), Some(parent), Some(name)) = (id, parent, new.file_name()) else {
            // Not indexed yet, or outside the coverage on one side.
            self.sync(old, false)?;
            self.sync(new, true)?;
            return Ok(true);
        };
        if let Some(existing) = resolve(&self.c, new)?
            && existing != id
        {
            self.remove(existing)?; // rename(2) replaced the target.
        }
        for (i, d) in self.deltas.drain().collect::<Vec<_>>() {
            self.c.prepare_cached("UPDATE nodes SET bytes=bytes+?2,blocks=blocks+?3,files=files+?4,entries=entries+?5 WHERE id=?1")?.execute(params![i,d.bytes,d.blocks,d.files,d.entries])?;
        }
        let (from, d, kind): (i64, Delta, i64) = self
            .c
            .prepare_cached("SELECT parent,bytes,blocks,files,entries,kind FROM nodes WHERE id=?1")?
            .query_row([id], |r| {
                Ok((
                    r.get(0)?,
                    Delta {
                        bytes: r.get(1)?,
                        blocks: r.get(2)?,
                        files: r.get(3)?,
                        entries: r.get(4)?,
                    },
                    r.get(5)?,
                ))
            })?;
        self.ancestors(
            from,
            Delta {
                bytes: -d.bytes,
                blocks: -d.blocks,
                files: -d.files,
                entries: -d.entries,
            },
        )?;
        self.c
            .prepare_cached("UPDATE nodes SET parent=?2,name=?3 WHERE id=?1")?
            .execute(params![id, parent, name.as_bytes()])?;
        if kind == 1 {
            self.parents.insert(id, parent);
        }
        self.ancestors(parent, d)?;
        self.dirty += 1;
        // Refresh the entry itself (rename updates ctime). If it has already
        // moved on again, a later rename in the same batch carries it further.
        if fs::symlink_metadata(new).is_ok() {
            self.sync(new, false)?;
        }
        self.maybe_commit()?;
        Ok(kind == 1)
    }
}
