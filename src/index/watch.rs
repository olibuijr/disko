use super::*;

impl Index {
    /// Paths in `watched` at or below `dir`.
    pub(super) fn watched_under(&self, dir: &Path) -> Vec<PathBuf> {
        self.watched
            .range(dir.to_owned()..)
            .take_while(|p| p.starts_with(dir))
            .cloned()
            .collect()
    }

    /// Forget watches below a mountpoint that was mounted or unmounted; the
    /// kernel already dropped them, so a later sweep must be free to re-add.
    pub fn forget_watches(&mut self, dir: &Path) {
        for p in self.watched_under(dir) {
            if let Some(w) = &mut self.watcher {
                let _ = w.unwatch(&p);
            }
            self.watched.remove(&p);
        }
    }

    /// Replace the skipped-mount set, emptying newly skipped mountpoints.
    pub fn set_skip(&mut self, skip: HashSet<PathBuf>) -> Result<()> {
        let added: Vec<_> = skip.difference(&self.skip).cloned().collect();
        self.skip = skip;
        for point in added {
            if let Some(id) = resolve(&self.c, &point)? {
                self.prune_children(id)?;
            }
            self.forget_watches(&point);
        }
        Ok(())
    }

    /// Install a directory watch unless a filesystem-wide fanotify mark already
    /// covers it. `m` is the directory's own metadata when the caller has it.
    pub(super) fn watch_dir(&mut self, p: &Path, m: Option<&fs::Metadata>) {
        if !self.filesystem_devices.is_empty() {
            let dev = match m {
                Some(m) => Some(m.dev()),
                None => fs::metadata(p).ok().map(|m| m.dev()),
            };
            if let Some(dev) = dev {
                if self.filesystem_devices.contains(&dev) {
                    return;
                }
                if nested_subvolume(p, &self.filesystem_devices) {
                    self.filesystem_devices.insert(dev);
                    return;
                }
            }
        }
        if self.watched.contains(p) {
            return;
        }
        if let Some(w) = &mut self.watcher {
            match w.watch(p, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    self.watched.insert(p.to_owned());
                }
                Err(e) => {
                    self.watch_errors += 1;
                    if self.watch_errors <= 5 {
                        eprintln!(
                            "watch: {}: {e}; periodic reconciliation remains active",
                            p.display()
                        );
                    }
                }
            }
        }
    }
}

/// A btrfs subvolume nested in a covered one shares its superblock, and so its
/// fanotify mark, but reports its own st_dev. Subvolume roots are inode 256;
/// a mount root with that inode is a different filesystem and is not covered.
fn nested_subvolume(p: &Path, covered: &HashSet<u64>) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Some(parent) = p.parent() else {
        return false;
    };
    if !fs::symlink_metadata(parent).is_ok_and(|m| covered.contains(&m.dev())) {
        return false;
    }
    let Ok(name) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stx = std::mem::MaybeUninit::<libc::statx>::uninit();
    if unsafe {
        libc::statx(
            libc::AT_FDCWD,
            name.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
            libc::STATX_INO,
            stx.as_mut_ptr(),
        )
    } != 0
    {
        return false;
    }
    let stx = unsafe { stx.assume_init() };
    let mount_root = libc::STATX_ATTR_MOUNT_ROOT as u64;
    stx.stx_ino == 256
        && stx.stx_attributes_mask & mount_root != 0
        && stx.stx_attributes & mount_root == 0
}
