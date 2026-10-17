use super::*;

/// A full scan keeps its pending directories in `scan_queue`, written in the
/// same transactions as the entries it indexes. A stop therefore leaves an
/// exact frontier, and the next run continues from it instead of rescanning.
impl Index {
    pub(super) fn queue_push(&self, job: &walk::Job) -> Result<()> {
        if self.resumable {
            self.c
                .prepare_cached("INSERT OR REPLACE INTO scan_queue VALUES(?1,?2)")?
                .execute(params![job.id, job.place.code()])?;
        }
        Ok(())
    }

    pub(super) fn queue_done(&self, id: i64) -> Result<()> {
        if self.resumable {
            self.c
                .prepare_cached("DELETE FROM scan_queue WHERE id=?1")?
                .execute([id])?;
        }
        Ok(())
    }

    /// The interrupted scan's pending directories that still belong in the index.
    fn pending(&mut self) -> Result<Vec<walk::Job>> {
        let rows = self
            .c
            .prepare("SELECT id,place FROM scan_queue")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut jobs = Vec::new();
        for (id, place) in rows {
            match path_of(&self.c, id) {
                Ok(path) if self.relevant(&path) && !self.skip.contains(&path) => {
                    jobs.push(walk::Job {
                        path,
                        id,
                        seen: self.next_seen(),
                        place: walk::Place::from_code(place),
                    });
                }
                _ => {
                    self.c.execute("DELETE FROM scan_queue WHERE id=?1", [id])?;
                }
            }
        }
        Ok(jobs)
    }

    /// Finish the pending directories, then walk the roots not yet started.
    fn walk_roots(&mut self, pending: Vec<walk::Job>) -> Result<()> {
        let mut first = 0;
        if !pending.is_empty() {
            eprintln!(
                "resuming interrupted scan: {} directories pending",
                pending.len()
            );
            self.drain(pending)?;
            first = self
                .c
                .query_row("SELECT value FROM meta WHERE key='scan_root'", [], |r| {
                    r.get::<_, String>(0)
                })
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .map_or(0, |i| i + 1);
        } else {
            self.c.execute("DELETE FROM scan_queue", [])?;
        }
        for (i, root) in self.roots.clone().iter().enumerate().skip(first) {
            set_meta(&self.c, "scan_root", i)?;
            self.sync(root, true)?;
        }
        Ok(())
    }

    pub fn reconcile(&mut self) -> Result<()> {
        let start_visited = self.visited;
        let start_errors = self.errors;
        let start_watch_errors = self.watch_errors;
        let pending = self.pending()?;
        let reason: String = if pending.is_empty() {
            self.c
                .query_row(
                    "SELECT value FROM meta WHERE key='recovery_reason'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| "startup".into())
        } else {
            "resumed after restart".into()
        };
        set_meta(&self.c, "recovery_reason", &reason)?;
        self.c.execute(
            "INSERT INTO scans(started,reason,state) VALUES(?1,?2,'scanning')",
            params![now(), reason],
        )?;
        let scan_id = self.c.last_insert_rowid();
        set_meta(&self.c, "scan_started", now())?;
        set_meta(&self.c, "scan_start_visited", start_visited)?;
        set_meta(&self.c, "state", "scanning")?;
        self.commit()?;
        let start_time = Instant::now();
        self.resumable = true;
        let walked = self.walk_roots(pending);
        self.resumable = false;
        walked?;
        set_meta(&self.c, "last_scan", now())?;
        self.c.execute("UPDATE scans SET finished=?2,visited=?3,errors=?4,watch_errors=?5,state='complete' WHERE id=?1",params![scan_id,now(),(self.visited-start_visited) as i64,(self.errors-start_errors) as i64,(self.watch_errors-start_watch_errors) as i64])?;
        self.c.execute(
            "DELETE FROM scans WHERE id NOT IN (SELECT id FROM scans ORDER BY id DESC LIMIT 100)",
            [],
        )?;
        set_meta(
            &self.c,
            "last_scan_seconds",
            start_time.elapsed().as_secs_f64(),
        )?;
        set_meta(
            &self.c,
            "state",
            if self.watcher.is_some() {
                "watching"
            } else {
                "idle"
            },
        )?;
        self.checkpoint()?;
        eprintln!(
            "indexed {} entries in {:.2}s; {} metadata errors, {} watch errors",
            self.visited,
            start_time.elapsed().as_secs_f64(),
            self.errors,
            self.watch_errors
        );
        Ok(())
    }
}
