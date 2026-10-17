use super::*;

impl Index {
    pub fn sync(&mut self, p: &Path, recursive: bool) -> Result<()> {
        if !self.relevant(p) {
            return Ok(());
        }
        let metadata = match fs::symlink_metadata(p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(id) = resolve(&self.c, p)? {
                    self.remove(id)?;
                }
                return Ok(());
            }
            Err(e) => {
                self.error(p, &e);
                return Ok(());
            }
            Ok(m) => m,
        };
        let Some(id) = self.ensure(p)? else {
            return Ok(());
        };
        if !recursive || !metadata.is_dir() {
            return Ok(());
        }
        self.sweep(p, id)
    }

    fn sweep(&mut self, p: &Path, root: i64) -> Result<()> {
        self.watch_dir(p, None);
        let mut pool = pool::Pool::start(
            self.stop.clone(),
            self.filter.clone(),
            self.jobs,
            walk::Job {
                path: p.to_owned(),
                id: root,
                seen: self.next_seen(),
                place: walk::Place::Regular,
            },
        )?;
        let mut in_flight = 1usize;
        while in_flight > 0 {
            let result = pool.receive()?;
            in_flight -= 1;
            if self.stop.load(std::sync::atomic::Ordering::Relaxed) {
                bail!("interrupted; completed batches are saved");
            }
            self.current_path = result.path.clone();
            in_flight += self.absorb(&mut pool, result)?;
        }
        Ok(())
    }

    /// Replay one enumerated directory: upsert children, enqueue new directories
    /// for the walk, then prune entries that disappeared since that generation.
    fn absorb(&mut self, pool: &mut pool::Pool, r: walk::DirResult) -> Result<usize> {
        let walk::DirResult {
            path,
            id,
            seen,
            place: inherited,
            complete,
            children,
            read_failed,
            entry_failed,
            stat_failed,
        } = r;
        let mut queued = 0usize;
        let root = path == Path::new("/");
        if let Some(e) = read_failed {
            self.error(&path, &e);
        }
        for e in entry_failed {
            self.error(&path, &e);
        }
        for (name, e) in stat_failed {
            self.c
                .prepare_cached("UPDATE nodes SET seen=?3 WHERE parent=?1 AND name=?2")?
                .execute(params![id, name.as_bytes(), seen])?;
            self.error(&path.join(&name), &e);
        }
        for (name, m) in children {
            let child = path.join(&name);
            let node = self.upsert(id, &name, &m, seen)?;
            if !m.is_dir() {
                continue;
            }
            let place = place_for(inherited, root, &name);
            self.watch_dir(&child, Some(&m));
            pool.dispatch(walk::Job {
                path: child,
                id: node,
                seen: self.next_seen(),
                place,
            });
            queued += 1;
        }
        if complete {
            let stale = self
                .c
                .prepare_cached("SELECT id FROM nodes WHERE parent=?1 AND seen!=?2")?
                .query_map(params![id, seen], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for node in stale {
                self.remove(node)?;
            }
        }
        Ok(queued)
    }

    fn next_seen(&mut self) -> i64 {
        self.generation += 1;
        self.generation
    }

    pub fn reconcile(&mut self) -> Result<()> {
        let start_visited = self.visited;
        let start_errors = self.errors;
        let start_watch_errors = self.watch_errors;
        let reason: String = self
            .c
            .query_row(
                "SELECT value FROM meta WHERE key='recovery_reason'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "startup".into());
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
        for root in self.roots.clone() {
            self.sync(&root, true)?;
        }
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

/// Keep the former root-first priority: home and var before the rest, /mnt last.
fn place_for(inherited: walk::Place, root: bool, name: &OsStr) -> walk::Place {
    if matches!(inherited, walk::Place::Tail) {
        return walk::Place::Tail;
    }
    if !root {
        return walk::Place::Regular;
    }
    match name.as_bytes() {
        b"home" | b"var" | b"usr" | b"tmp" => walk::Place::Front,
        b"mnt" => walk::Place::Tail,
        _ => walk::Place::Regular,
    }
}
