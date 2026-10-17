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
        if self.skip.contains(p) {
            return self.prune_children(id);
        }
        self.sweep(p, id)
    }

    fn sweep(&mut self, p: &Path, root: i64) -> Result<()> {
        self.watch_dir(p, None);
        let job = walk::Job {
            path: p.to_owned(),
            id: root,
            seen: self.next_seen(),
            place: walk::Place::Regular,
        };
        self.queue_push(&job)?;
        self.drain(vec![job])
    }

    /// Walk the given directories and everything below them.
    pub(super) fn drain(&mut self, jobs: Vec<walk::Job>) -> Result<()> {
        let mut jobs = jobs.into_iter();
        let Some(first) = jobs.next() else {
            return Ok(());
        };
        let mut pool = pool::Pool::start(self.stop.clone(), self.filter.clone(), self.jobs, first)?;
        let mut in_flight = 1usize;
        for job in jobs {
            pool.dispatch(job);
            in_flight += 1;
        }
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

    /// Replay one enumerated directory against its indexed children: write only
    /// new or changed entries, enqueue subdirectories, and remove names that are
    /// gone. Unchanged entries cost no writes, which keeps rescans cheap.
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
        let mut known = self.children_of(id)?;
        for (name, e) in stat_failed {
            // Preserve old information on permission/race errors; never infer deletion.
            known.remove(name.as_bytes());
            self.error(&path.join(&name), &e);
        }
        for (name, m) in children {
            let child = path.join(&name);
            let node = match known.remove(name.as_bytes()) {
                Some(k) if k.matches(&m) => {
                    self.visited += 1;
                    if m.is_dir() {
                        self.parents.insert(k.id, id);
                    }
                    k.id
                }
                _ => self.upsert(id, &name, &m, seen)?,
            };
            if !m.is_dir() {
                continue;
            }
            if self.skip.contains(&child) {
                // Contents are counted where the filesystem is mounted elsewhere.
                self.prune_children(node)?;
                continue;
            }
            let place = walk::place_for(inherited, root, &name);
            self.watch_dir(&child, Some(&m));
            let job = walk::Job {
                path: child,
                id: node,
                seen: self.next_seen(),
                place,
            };
            self.queue_push(&job)?;
            pool.dispatch(job);
            queued += 1;
        }
        if complete {
            for gone in known.into_values() {
                self.remove(gone.id)?;
            }
        }
        self.queue_done(id)?;
        // Unchanged entries never reach upsert; keep heartbeats and batches flowing.
        self.maybe_commit()?;
        Ok(queued)
    }

    pub(super) fn next_seen(&mut self) -> i64 {
        self.generation += 1;
        self.generation
    }
}
