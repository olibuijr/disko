use anyhow::{Result, bail};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ffi::{CStr, CString, OsString},
    fs::File,
    io,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::MetadataExt,
        },
    },
    path::{Path, PathBuf},
};

/// A directory on a marked filesystem used to decode event file handles.
/// `map` is empty when decoded paths are already real paths; for a detached
/// btrfs top-level mount it translates subvolume paths to their mountpoints.
pub(super) struct Source {
    root: File,
    map: Vec<(PathBuf, PathBuf)>,
}
pub(super) type Handles = HashMap<[u8; 8], Vec<Source>>;

struct Mount {
    root: PathBuf,
    point: PathBuf,
    fstype: String,
    source: String,
}

pub(super) fn mark(
    fd: i32,
    roots: &[PathBuf],
    excludes: &[PathBuf],
) -> Result<(Handles, HashSet<u64>)> {
    let mounts = mountinfo(&std::fs::read_to_string("/proc/self/mountinfo")?);
    let mut paths = roots.to_vec();
    for m in &mounts {
        if roots.iter().any(|r| m.point.starts_with(r))
            && !excludes.iter().any(|p| m.point.starts_with(p))
        {
            paths.push(m.point.clone());
        }
    }
    paths.sort();
    paths.dedup();
    let mut handles = Handles::new();
    let mut devices = HashSet::new();
    let mut btrfs = BTreeSet::new();
    let mask = libc::FAN_MODIFY
        | libc::FAN_ATTRIB
        | libc::FAN_CLOSE_WRITE
        | libc::FAN_CREATE
        | libc::FAN_DELETE
        | libc::FAN_MOVED_FROM
        | libc::FAN_MOVED_TO
        | libc::FAN_ONDIR;
    for path in paths {
        if path.components().any(|c| c.as_os_str() == ".snapshots") {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let name = CString::new(path.as_os_str().as_bytes())?;
        if unsafe {
            libc::fanotify_mark(
                fd,
                libc::FAN_MARK_ADD | libc::FAN_MARK_FILESYSTEM,
                mask,
                libc::AT_FDCWD,
                name.as_ptr(),
            )
        } != 0
        {
            // btrfs refuses filesystem marks on subvolumes other than the top level.
            if io::Error::last_os_error().raw_os_error() == Some(libc::EXDEV)
                && let Some(m) = containing(&mounts, &path)
                && m.fstype == "btrfs"
            {
                btrfs.insert(m.source.clone());
            }
            continue;
        }
        let Some(fsid) = fsid(&file) else {
            continue;
        };
        handles.entry(fsid).or_default().push(Source {
            root: file,
            map: Vec::new(),
        });
        devices.insert(metadata.dev());
    }
    for source in btrfs {
        let root = match whole_btrfs(fd, &source, mask) {
            Ok(root) => root,
            Err(e) => {
                eprintln!("fanotify: btrfs {source}: {e}; using directory inotify watches there");
                continue;
            }
        };
        let Some(fsid) = fsid(&root) else {
            continue;
        };
        let map: Vec<_> = mounts
            .iter()
            .filter(|m| m.fstype == "btrfs" && m.source == source)
            .map(|m| (m.root.clone(), m.point.clone()))
            .collect();
        for (_, point) in &map {
            if let Ok(m) = std::fs::metadata(point) {
                devices.insert(m.dev());
            }
        }
        handles.entry(fsid).or_default().push(Source { root, map });
    }
    Ok((handles, devices))
}

/// Mark a whole btrfs filesystem through a private, read-only, detached mount
/// of its top-level subvolume. Nothing is attached to the mount tree; the mount
/// lives exactly as long as the returned directory handle.
fn whole_btrfs(fd: i32, source: &str, mask: u64) -> Result<File> {
    const FSOPEN_CLOEXEC: libc::c_uint = 1;
    const FSCONFIG_SET_STRING: libc::c_uint = 1;
    const FSCONFIG_CMD_CREATE: libc::c_uint = 6;
    const FSMOUNT_CLOEXEC: libc::c_uint = 1;
    const MOUNT_ATTR_RDONLY: libc::c_uint = 1;
    fn owned(raw: libc::c_long, what: &str) -> Result<OwnedFd> {
        if raw < 0 {
            bail!("{what}: {}", io::Error::last_os_error());
        }
        Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
    }
    let context = owned(
        unsafe { libc::syscall(libc::SYS_fsopen, c"btrfs".as_ptr(), FSOPEN_CLOEXEC) },
        "fsopen",
    )?;
    let source = CString::new(source)?;
    let settings: [(&CStr, &CStr); 2] = [(c"source", &source), (c"subvolid", c"5")];
    for (key, value) in settings {
        if unsafe {
            libc::syscall(
                libc::SYS_fsconfig,
                context.as_raw_fd(),
                FSCONFIG_SET_STRING,
                key.as_ptr(),
                value.as_ptr(),
                0,
            )
        } < 0
        {
            bail!("fsconfig {key:?}: {}", io::Error::last_os_error());
        }
    }
    if unsafe {
        libc::syscall(
            libc::SYS_fsconfig,
            context.as_raw_fd(),
            FSCONFIG_CMD_CREATE,
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(),
            0,
        )
    } < 0
    {
        bail!("fsconfig create: {}", io::Error::last_os_error());
    }
    let mount = owned(
        unsafe {
            libc::syscall(
                libc::SYS_fsmount,
                context.as_raw_fd(),
                FSMOUNT_CLOEXEC,
                MOUNT_ATTR_RDONLY,
            )
        },
        "fsmount",
    )?;
    // fsmount returns an O_PATH descriptor, which fanotify_mark and
    // open_by_handle_at reject; open the mount root as a real directory.
    let root = unsafe {
        libc::openat(
            mount.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if root < 0 {
        bail!("open top-level subvolume: {}", io::Error::last_os_error());
    }
    let root = unsafe { File::from_raw_fd(root) };
    if unsafe {
        libc::fanotify_mark(
            fd,
            libc::FAN_MARK_ADD | libc::FAN_MARK_FILESYSTEM,
            mask,
            root.as_raw_fd(),
            c".".as_ptr(),
        )
    } != 0
    {
        bail!("fanotify_mark: {}", io::Error::last_os_error());
    }
    Ok(root)
}

fn fsid(file: &File) -> Option<[u8; 8]> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    unsafe { std::slice::from_raw_parts((&stat.f_fsid as *const libc::fsid_t).cast::<u8>(), 8) }
        .try_into()
        .ok()
}

fn mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            let split = fields.iter().position(|s| *s == "-")?;
            if split < 5 || fields.len() < split + 3 {
                return None;
            }
            Some(Mount {
                root: unescape(fields[3]),
                point: unescape(fields[4]),
                fstype: fields[split + 1].to_owned(),
                source: unescape(fields[split + 2]).to_string_lossy().into_owned(),
            })
        })
        .collect()
}

/// mountinfo escapes space, tab, newline and backslash as three-digit octal.
fn unescape(field: &str) -> PathBuf {
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
fn containing<'a>(mounts: &'a [Mount], path: &Path) -> Option<&'a Mount> {
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

/// Map a path inside the filesystem to where it is mounted, preferring the
/// most specific subvolume.
fn translate(map: &[(PathBuf, PathBuf)], path: &Path) -> Option<PathBuf> {
    map.iter()
        .filter_map(|(root, point)| {
            let rest = path.strip_prefix(root).ok()?;
            Some((root.as_os_str().len(), point.join(rest)))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

pub(super) fn resolve(handles: &Handles, fsid: [u8; 8], handle: &[u8]) -> Option<PathBuf> {
    // Copy into aligned storage before passing the variable-size file_handle to libc.
    let mut aligned = vec![0_u64; handle.len().div_ceil(8)];
    unsafe {
        std::ptr::copy_nonoverlapping(
            handle.as_ptr(),
            aligned.as_mut_ptr().cast::<u8>(),
            handle.len(),
        );
    }
    for source in handles.get(&fsid)? {
        let raw = unsafe {
            libc::open_by_handle_at(
                source.root.as_raw_fd(),
                aligned.as_mut_ptr().cast(),
                libc::O_PATH | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            continue;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let Ok(path) = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())) else {
            continue;
        };
        if !path.is_absolute() || path.as_os_str().as_bytes().ends_with(b" (deleted)") {
            continue;
        }
        if source.map.is_empty() {
            return Some(path);
        }
        if let Some(path) = translate(&source.map, &path) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
23 1 0:31 /@ / rw,relatime shared:1 - btrfs /dev/mapper/root rw,subvolid=256,subvol=/@
45 23 0:30 /@home /home rw,relatime shared:2 - btrfs /dev/mapper/root rw,subvolid=257,subvol=/@home
46 23 0:57 /@log /var/log rw,relatime shared:3 - btrfs /dev/mapper/root rw,subvolid=258,subvol=/@log
47 23 259:2 / /mnt/my\\040disk rw,relatime shared:4 - ext4 /dev/nvme0n1p2 rw";

    #[test]
    fn parses_mountinfo_with_escapes() {
        let mounts = mountinfo(SAMPLE);
        assert_eq!(mounts.len(), 4);
        assert_eq!(mounts[1].root, Path::new("/@home"));
        assert_eq!(mounts[1].point, Path::new("/home"));
        assert_eq!(mounts[3].point, Path::new("/mnt/my disk"));
        assert_eq!(mounts[3].fstype, "ext4");
        assert_eq!(mounts[0].source, "/dev/mapper/root");
        assert_eq!(unescape("a\\134b\\011c"), Path::new("a\\b\tc"));
        assert_eq!(unescape("trailing\\04"), Path::new("trailing\\04"));
    }

    #[test]
    fn finds_containing_mount_and_translates_subvolumes() {
        let mounts = mountinfo(SAMPLE);
        assert_eq!(
            containing(&mounts, Path::new("/home/user/x"))
                .unwrap()
                .point,
            Path::new("/home")
        );
        assert_eq!(
            containing(&mounts, Path::new("/var/lib")).unwrap().point,
            Path::new("/")
        );
        let map: Vec<_> = mounts
            .iter()
            .filter(|m| m.fstype == "btrfs")
            .map(|m| (m.root.clone(), m.point.clone()))
            .collect();
        let t = |p: &str| translate(&map, Path::new(p));
        assert_eq!(t("/@home/user/x").unwrap(), Path::new("/home/user/x"));
        assert_eq!(t("/@/var/tmp").unwrap(), Path::new("/var/tmp"));
        assert_eq!(t("/@log").unwrap(), Path::new("/var/log"));
        assert_eq!(t("/@/swap/file").unwrap(), Path::new("/swap/file"));
        assert_eq!(t("/@homework/x"), None); // component-wise, not string prefix
        assert_eq!(t("/@factory/etc"), None); // unmounted subvolume
    }
}
