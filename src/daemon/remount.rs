use super::*;

/// Below this much free space on the database's filesystem, full scans wait;
/// a scan that hits a full disk fails and costs a restart plus a rescan.
pub(super) const MIN_FREE_BYTES: u64 = 1 << 30;

pub(super) fn low_space(db: &Path) -> bool {
    db.parent()
        .and_then(|p| fs2::available_space(p).ok())
        .is_some_and(|free| free < MIN_FREE_BYTES)
}

pub(super) fn activity(idx: &Index, path: &Path, kind: &str) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    idx.c.execute(
        "INSERT INTO activity(ts,path,kind) VALUES(?1,?2,?3)",
        params![now(), path.as_os_str().as_bytes(), kind],
    )?;
    Ok(())
}

/// Something was mounted or unmounted: mark new filesystems, update skipped
/// duplicates, and resync each changed mountpoint, which now shows either the
/// new filesystem or the directory underneath.
pub(super) fn remount(
    idx: &mut Index,
    fan: Option<&fanotify::Fanotify>,
    before: &[mounts::Mount],
    after: &[mounts::Mount],
) -> Result<()> {
    if let Some(fan) = fan {
        idx.filesystem_devices = fan.refresh(after);
    }
    idx.set_skip(mounts::duplicates(after))?;
    let changed: Vec<(PathBuf, &str)> = after
        .iter()
        .filter(|m| !before.iter().any(|b| b.id == m.id))
        .map(|m| (m.point.clone(), "mount"))
        .chain(
            before
                .iter()
                .filter(|b| !after.iter().any(|m| m.id == b.id))
                .map(|b| (b.point.clone(), "unmount")),
        )
        .collect();
    for (point, kind) in changed {
        if !idx.relevant(&point) {
            continue;
        }
        idx.forget_watches(&point);
        idx.sync(&point, true)?;
        activity(idx, &point, kind)?;
    }
    idx.commit()
}
