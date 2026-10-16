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
        let mut stack = vec![(p.to_owned(), id)];
        while let Some((path, parent)) = stack.pop() {
            self.current_path = path.clone();
            if self.stop.load(std::sync::atomic::Ordering::Relaxed) {
                bail!("interrupted; completed batches are saved");
            }
            self.watch_dir(&path); // Install before enumeration to close the scan/watch gap.
            let rd = match fs::read_dir(&path) {
                Ok(rd) => rd,
                Err(e) => {
                    self.error(&path, &e);
                    continue;
                }
            };
            self.generation += 1;
            let generation = self.generation;
            let mut complete = true;
            for entry in rd {
                let entry = match entry {
                    Ok(v) => v,
                    Err(e) => {
                        self.error(&path, &e);
                        complete = false;
                        continue;
                    }
                };
                let child = entry.path();
                if self.excluded(&child) {
                    continue;
                }
                match fs::symlink_metadata(&child) {
                    Ok(m) => {
                        let id = self.upsert(parent, &entry.file_name(), &m, generation)?;
                        if m.is_dir() {
                            stack.push((child, id));
                        }
                    }
                    Err(e) => {
                        // Preserve old information on permission/race errors; never infer deletion.
                        self.c
                            .prepare_cached("UPDATE nodes SET seen=?3 WHERE parent=?1 AND name=?2")?
                            .execute(params![parent, entry.file_name().as_bytes(), generation])?;
                        self.error(&child, &e);
                    }
                }
            }
            if path == Path::new("/") {
                stack.sort_by_key(|(p, _)| match p.to_str() {
                    Some("/home") => 10,
                    Some("/var") => 9,
                    Some("/usr") => 8,
                    Some("/tmp") => 7,
                    Some("/mnt") => -10,
                    _ => 0,
                });
            }
            if complete {
                let stale = self
                    .c
                    .prepare_cached("SELECT id FROM nodes WHERE parent=?1 AND seen!=?2")?
                    .query_map(params![parent, generation], |r| r.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for id in stale {
                    self.remove(id)?;
                }
            }
        }
        Ok(())
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
        let start = Instant::now();
        for root in self.roots.clone() {
            self.sync(&root, true)?;
        }
        set_meta(&self.c, "last_scan", now())?;
        self.c.execute("UPDATE scans SET finished=?2,visited=?3,errors=?4,watch_errors=?5,state='complete' WHERE id=?1",params![scan_id,now(),(self.visited-start_visited) as i64,(self.errors-start_errors) as i64,(self.watch_errors-start_watch_errors) as i64])?;
        self.c.execute(
            "DELETE FROM scans WHERE id NOT IN (SELECT id FROM scans ORDER BY id DESC LIMIT 100)",
            [],
        )?;
        set_meta(&self.c, "last_scan_seconds", start.elapsed().as_secs_f64())?;
        set_meta(
            &self.c,
            "state",
            if self.watcher.is_some() {
                "watching"
            } else {
                "idle"
            },
        )?;
        self.commit()?;
        eprintln!(
            "indexed {} entries in {:.2}s; {} metadata errors, {} watch errors",
            self.visited,
            start.elapsed().as_secs_f64(),
            self.errors,
            self.watch_errors
        );
        Ok(())
    }
}
