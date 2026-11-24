use super::*;
use crate::mounts::{self, DISK_TYPES, Mount};
use std::{collections::BTreeMap, ffi::CString, os::unix::ffi::OsStrExt};

/// One entry per filesystem (not per mount: Btrfs subvolumes share capacity),
/// with what the index can see on it. `unindexed` is space in use (bytes) that
/// no indexed path accounts for: snapshots, excluded trees, metadata and slack,
/// offset by compression and deduplication. It is an estimate, not a size of
/// anything that can be deleted directly. `indexed` counts every path, so
/// hardlinks, reflinks and compression can push it past `used`; `unindexed`
/// is then 0, never negative.
pub fn filesystems(c: &Connection) -> Result<Vec<Value>> {
    let table = mounts::read();
    let mut groups: BTreeMap<String, Vec<&Mount>> = BTreeMap::new();
    for m in table
        .iter()
        .filter(|m| DISK_TYPES.contains(&m.fstype.as_str()))
    {
        groups.entry(m.group()).or_default().push(m);
    }
    let mut out = Vec::new();
    for members in groups.values() {
        // Outermost mountpoints of this filesystem that the index contains.
        let mut tops: Vec<(&Mount, i64)> = Vec::new();
        for m in members {
            if members
                .iter()
                .any(|o| o.point != m.point && m.point.starts_with(&o.point))
            {
                continue;
            }
            if let Some(id) = resolve(c, &m.point)? {
                tops.push((m, id));
            }
        }
        let Some(&(primary, primary_id)) =
            tops.iter().min_by_key(|(m, _)| m.point.as_os_str().len())
        else {
            continue;
        };
        let Some((capacity, available, used)) = usage(&primary.point) else {
            continue;
        };
        let mut indexed = 0_i64;
        for (top, id) in &tops {
            indexed += blocks(c, *id)?;
            // Other filesystems mounted below this one are counted on their own.
            let foreign: Vec<&Mount> = table
                .iter()
                .filter(|f| {
                    f.group() != top.group()
                        && f.point != top.point
                        && f.point.starts_with(&top.point)
                })
                .collect();
            for f in &foreign {
                let nested = foreign
                    .iter()
                    .any(|o| o.point != f.point && f.point.starts_with(&o.point));
                if !nested && let Some(fid) = resolve(c, &f.point)? {
                    indexed -= blocks(c, fid)?;
                }
            }
        }
        let path = primary.point.to_string_lossy().into_owned();
        out.push(json!({
            "id": primary_id,
            "path": path,
            "name": if path == "/" { "System disk".to_owned() } else {
                primary.point.file_name().unwrap_or_default().to_string_lossy().into_owned()
            },
            "device": primary.source,
            "filesystem": primary.fstype,
            "mounts": members.iter().map(|m| m.point.to_string_lossy()).collect::<Vec<_>>(),
            "capacity": capacity,
            "available": available,
            "used": used,
            "indexed": indexed.max(0),
            "unindexed": unindexed(used, indexed),
        }));
    }
    Ok(out)
}

/// Used space the index does not account for, never below zero.
fn unindexed(used: u64, indexed: i64) -> i64 {
    (used as i64).saturating_sub(indexed.max(0)).max(0)
}

fn blocks(c: &Connection, id: i64) -> Result<i64> {
    Ok(c.query_row("SELECT blocks FROM nodes WHERE id=?1", [id], |r| r.get(0))?)
}

/// Capacity, space available to unprivileged users, and space in use.
// statvfs field widths vary by target; the casts are no-ops only on 64-bit.
#[allow(clippy::unnecessary_cast)]
fn usage(path: &Path) -> Option<(u64, u64, u64)> {
    let name = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(name.as_ptr(), s.as_mut_ptr()) } != 0 {
        return None;
    }
    let s = unsafe { s.assume_init() };
    let unit = s.f_frsize as u64;
    Some((
        s.f_blocks as u64 * unit,
        s.f_bavail as u64 * unit,
        (s.f_blocks - s.f_bfree) as u64 * unit,
    ))
}

#[cfg(test)]
mod tests {
    use super::unindexed;

    #[test]
    fn unindexed_space_is_never_negative() {
        assert_eq!(unindexed(1000, 600), 400);
        // Hardlinks are counted once per path, so the index can exceed use.
        assert_eq!(unindexed(1000, 1500), 0);
        assert_eq!(unindexed(1000, -5), 1000);
    }
}
