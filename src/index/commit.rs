use super::*;

impl Index {
    pub(super) fn ancestors(&mut self, mut id: i64, d: Delta) -> Result<()> {
        while id > 0 {
            self.deltas.entry(id).or_default().add(d);
            id = match self.parents.get(&id) {
                Some(p) => *p,
                None => {
                    let p = self
                        .c
                        .prepare_cached("SELECT parent FROM nodes WHERE id=?1")?
                        .query_row([id], |r| r.get(0))
                        .optional()?
                        .unwrap_or(0);
                    self.parents.insert(id, p);
                    p
                }
            };
        }
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        {
            let mut stmt=self.c.prepare_cached("UPDATE nodes SET bytes=bytes+?2,blocks=blocks+?3,files=files+?4,entries=entries+?5 WHERE id=?1")?;
            for (id, d) in self.deltas.drain() {
                stmt.execute(params![id, d.bytes, d.blocks, d.files, d.entries])?;
            }
        }
        set_meta(&self.c, "visited_this_run", self.visited)?;
        set_meta(&self.c, "metadata_errors", self.errors)?;
        set_meta(&self.c, "watch_errors", self.watch_errors)?;
        set_meta(
            &self.c,
            "watches",
            self.watched.len() + self.filesystem_devices.len(),
        )?;
        set_meta(
            &self.c,
            "watch_backend",
            if self.filesystem_devices.is_empty() {
                "inotify"
            } else {
                "fanotify + inotify fallback"
            },
        )?;
        set_meta(&self.c, "heartbeat", now())?;
        set_meta(&self.c, "scan_path", self.current_path.to_string_lossy())?;
        if now() - self.last_sample >= 30 {
            self.c.execute("INSERT OR REPLACE INTO samples SELECT ?1,entries,files,bytes,blocks,?2,?3,coalesce((SELECT value FROM meta WHERE key='state'),'idle') FROM nodes WHERE id=1",params![now(),self.visited as i64,(self.watched.len()+self.filesystem_devices.len()) as i64])?;
            self.c
                .execute("DELETE FROM samples WHERE ts<?1", [now() - 7 * 86400])?;
            self.last_sample = now();
        }
        self.c.execute_batch("COMMIT; BEGIN IMMEDIATE")?;
        self.dirty = 0;
        self.last_commit = Instant::now();
        Ok(())
    }

    pub(super) fn maybe_commit(&mut self) -> Result<()> {
        if self.dirty >= 4096 || self.last_commit.elapsed() > Duration::from_secs(2) {
            self.commit()?;
        }
        Ok(())
    }
}
