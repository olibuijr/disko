use super::*;

/// Rows deleted per garbage-collection step. Each step is followed by a commit,
/// so a large deleted tree never holds the writer in one long transaction.
pub const GC_ROWS: usize = 2048;

impl Index {
    /// Unlink a subtree in constant time: its ancestors lose its totals and
    /// readers stop reaching it at once, while the rows themselves are deleted
    /// later by `collect_garbage`. A detached root has `parent = -id`, which
    /// keeps `UNIQUE(parent,name)` free for a replacement entry.
    pub(super) fn detach(&mut self, id: i64, parent: i64, d: Delta) -> Result<()> {
        self.c
            .prepare_cached("UPDATE nodes SET parent=?2 WHERE id=?1")?
            .execute(params![id, -id])?;
        self.parents.remove(&id);
        self.ancestors(parent, d)?;
        self.dirty += 1;
        self.garbage = true;
        Ok(())
    }

    /// Delete up to `budget` detached rows, children before their parents, so an
    /// interrupted collection still leaves every remaining row below a detached
    /// root. Returns true while garbage remains.
    pub fn collect_garbage(&mut self, budget: usize) -> Result<bool> {
        if !self.garbage {
            return Ok(false);
        }
        let mut left = budget;
        while left > 0 {
            let root: Option<i64> = self
                .c
                .prepare_cached("SELECT id FROM nodes WHERE parent<0 LIMIT 1")?
                .query_row([], |r| r.get(0))
                .optional()?;
            let Some(root) = root else {
                self.garbage = false;
                break;
            };
            let mut stack = vec![root];
            while let Some(&top) = stack.last() {
                let sub: Option<i64> = self
                    .c
                    .prepare_cached("SELECT id FROM nodes WHERE parent=?1 AND kind=1 LIMIT 1")?
                    .query_row([top], |r| r.get(0))
                    .optional()?;
                if let Some(sub) = sub {
                    stack.push(sub);
                    continue;
                }
                let gone = self
                    .c
                    .prepare_cached(
                        "DELETE FROM nodes WHERE id IN (SELECT id FROM nodes WHERE parent=?1 LIMIT ?2)",
                    )?
                    .execute(params![top, left as i64])?;
                left = left.saturating_sub(gone);
                if left == 0 {
                    break; // `top` may still have children; the next step continues.
                }
                self.c
                    .prepare_cached("DELETE FROM nodes WHERE id=?1")?
                    .execute([top])?;
                left -= 1;
                stack.pop();
                if left == 0 {
                    break;
                }
            }
        }
        // Deleted row IDs can be reused, so cached parent links may now be wrong.
        self.parents.clear();
        crate::index::progress();
        Ok(self.garbage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_trees_are_collected_children_first_in_bounded_steps() {
        let dir = tempfile::tempdir().unwrap();
        let c = open(&dir.path().join("t.db"), true).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut idx = Index::new(c, vec!["/".into()], vec![], stop, 1).unwrap();
        // /a with 3 subfolders of 5 files each: 1 + 3 + 15 rows.
        let row = |idx: &Index, parent: i64, name: &str, kind: i64| -> i64 {
            idx.c.query_row("INSERT INTO nodes(parent,name,kind,size,allocated,bytes,blocks,files,entries,mtime,ctime,dev,ino,links,mode,uid,gid,seen) VALUES(?1,?2,?3,1,1,1,1,?4,1,0,0,0,0,1,0,0,0,0) RETURNING id", params![parent, name.as_bytes(), kind, i64::from(kind == 0)], |r| r.get(0)).unwrap()
        };
        let a = row(&idx, 1, "a", 1);
        for s in 0..3 {
            let sub = row(&idx, a, &format!("s{s}"), 1);
            for f in 0..5 {
                row(&idx, sub, &format!("f{f}"), 0);
            }
        }
        idx.remove(a).unwrap();
        assert_eq!(resolve(&idx.c, Path::new("/a")).unwrap(), None);
        let count = |idx: &Index| -> i64 {
            idx.c
                .query_row("SELECT count(*) FROM nodes", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(&idx), 20);
        let mut steps = 0;
        while idx.collect_garbage(4).unwrap() {
            steps += 1;
            // Nothing is ever orphaned: every row reaches the root or a detached root.
            let orphans: i64 = idx.c.query_row("SELECT count(*) FROM nodes n WHERE n.parent>0 AND NOT EXISTS (SELECT 1 FROM nodes p WHERE p.id=n.parent)", [], |r| r.get(0)).unwrap();
            assert_eq!(orphans, 0);
        }
        assert!(steps >= 4, "{steps}");
        assert_eq!(count(&idx), 1);
    }
}
