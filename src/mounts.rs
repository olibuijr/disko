//! Parsed `/proc/self/mountinfo`, shared by the watcher, the scanner and disk queries.
use std::{
    collections::HashSet,
    ffi::OsString,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};

/// Filesystems that store data on a device, as opposed to virtual or overlay views.
pub const DISK_TYPES: &[&str] = &[
    "btrfs", "ext4", "ext3", "xfs", "f2fs", "ntfs3", "exfat", "vfat", "fuseblk", "zfs",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub id: u64,
    pub device: String,
    pub root: PathBuf,
    pub point: PathBuf,
    pub fstype: String,
    pub source: String,
}

impl Mount {
    /// Mounts sharing a group share one filesystem. Btrfs subvolumes report a
    /// distinct device number each, so Btrfs groups by its source device.
    pub fn group(&self) -> String {
        if self.fstype == "btrfs" {
            format!("btrfs:{}", self.source)
        } else {
            self.device.clone()
        }
    }
}

pub fn read() -> Vec<Mount> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .map(|t| parse(&t))
        .unwrap_or_default()
}

pub fn parse(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            let split = fields.iter().position(|s| *s == "-")?;
            if split < 5 || fields.len() < split + 3 {
                return None;
            }
            Some(Mount {
                id: fields[0].parse().ok()?,
                device: fields[2].to_owned(),
                root: unescape(fields[3]),
                point: unescape(fields[4]),
                fstype: fields[split + 1].to_owned(),
                source: unescape(fields[split + 2]).to_string_lossy().into_owned(),
            })
        })
        .collect()
}

/// mountinfo escapes space, tab, newline and backslash as three-digit octal.
pub fn unescape(field: &str) -> PathBuf {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            out.push(
                (bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0'),
            );
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    PathBuf::from(OsString::from_vec(out))
}

/// The topmost mount whose mountpoint contains `path`.
pub fn containing<'a>(mounts: &'a [Mount], path: &Path) -> Option<&'a Mount> {
    let mut best: Option<&Mount> = None;
    for m in mounts {
        if path.starts_with(&m.point)
            && best.is_none_or(|b| m.point.as_os_str().len() >= b.point.as_os_str().len())
        {
            best = Some(m);
        }
    }
    best
}

/// Mountpoints whose contents are already counted elsewhere: overlay views
/// (container roots) and bind mounts that expose a directory another mount of
/// the same filesystem already shows at a different path. The scanner records
/// these mountpoints but does not descend into them.
pub fn duplicates(mounts: &[Mount]) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    for (i, b) in mounts.iter().enumerate() {
        if b.fstype == "overlay" {
            out.insert(b.point.clone());
            continue;
        }
        let shown_elsewhere = mounts[..i].iter().any(|a| {
            a.group() == b.group()
                && a.point != b.point
                && b.root
                    .strip_prefix(&a.root)
                    .is_ok_and(|rest| a.point.join(rest) != b.point)
                && !out.contains(&a.point)
        });
        if shown_elsewhere {
            out.insert(b.point.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE: &str = "\
23 1 0:31 /@ / rw,relatime shared:1 - btrfs /dev/mapper/root rw,subvolid=256,subvol=/@
45 23 0:30 /@home /home rw,relatime shared:2 - btrfs /dev/mapper/root rw,subvolid=257,subvol=/@home
46 23 0:57 /@log /var/log rw,relatime shared:3 - btrfs /dev/mapper/root rw,subvolid=258,subvol=/@log
47 23 259:2 / /mnt/my\\040disk rw,relatime shared:4 - ext4 /dev/nvme0n1p2 rw
48 23 0:31 /@/srv/data /data rw,relatime shared:5 - btrfs /dev/mapper/root rw,subvolid=256,subvol=/@
49 23 0:30 /@home/user /home/user rw,relatime shared:6 - btrfs /dev/mapper/root rw,subvolid=257,subvol=/@home
50 23 0:90 / /var/lib/docker/rootfs/overlayfs/abc rw - overlay overlay rw,lowerdir=x
51 23 259:2 /photos /pics rw,relatime shared:7 - ext4 /dev/nvme0n1p2 rw";

    #[test]
    fn parses_mountinfo_with_escapes() {
        let mounts = parse(SAMPLE);
        assert_eq!(mounts.len(), 8);
        assert_eq!(mounts[1].root, Path::new("/@home"));
        assert_eq!(mounts[1].point, Path::new("/home"));
        assert_eq!(mounts[3].point, Path::new("/mnt/my disk"));
        assert_eq!(mounts[3].fstype, "ext4");
        assert_eq!(mounts[0].source, "/dev/mapper/root");
        assert_eq!(mounts[0].group(), mounts[1].group());
        assert_ne!(mounts[0].group(), mounts[3].group());
        assert_eq!(unescape("a\\134b\\011c"), Path::new("a\\b\tc"));
        assert_eq!(unescape("trailing\\04"), Path::new("trailing\\04"));
    }

    #[test]
    fn finds_containing_mount() {
        let mounts = parse(SAMPLE);
        let at = |p: &str| containing(&mounts, Path::new(p)).unwrap().point.clone();
        assert_eq!(at("/home/other/x"), Path::new("/home"));
        assert_eq!(at("/var/lib"), Path::new("/"));
    }

    #[test]
    fn skips_overlays_and_relocated_binds_only() {
        let skip = duplicates(&parse(SAMPLE));
        // /data shows /srv/data, already visible below /.
        assert!(skip.contains(Path::new("/data")));
        // /pics shows /photos of the ext4 disk mounted at /mnt/my disk.
        assert!(skip.contains(Path::new("/pics")));
        assert!(skip.contains(Path::new("/var/lib/docker/rootfs/overlayfs/abc")));
        // Subvolumes and a bind that shadows the same path are not duplicates.
        assert!(!skip.contains(Path::new("/home")));
        assert!(!skip.contains(Path::new("/home/user")));
        assert!(!skip.contains(Path::new("/var/log")));
        assert!(!skip.contains(Path::new("/mnt/my disk")));
        assert_eq!(skip.len(), 3);
    }
}
