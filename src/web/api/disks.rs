use super::*;
use std::collections::HashSet;
pub(super) fn roots(c: &Connection) -> Result<Value> {
    let coverage: String = c.query_row("SELECT value FROM meta WHERE key='coverage'", [], |r| {
        r.get(0)
    })?;
    let coverage: Value = serde_json::from_str(&coverage)?;
    let mut indexes = Vec::new();
    for p in coverage["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if let Some(id) = resolve(c, Path::new(p))? {
            indexes.push(json!({"id":id,"path":p,"name":if p=="/"{"Machine index"}else{p}}));
        }
    }
    let mut disks = Vec::new();
    let mut devices = HashSet::new();
    for line in std::fs::read_to_string("/proc/self/mountinfo")?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(split) = fields.iter().position(|s| *s == "-") else {
            continue;
        };
        if fields.len() < split + 3
            || ![
                "btrfs", "ext4", "ext3", "xfs", "f2fs", "ntfs3", "exfat", "vfat", "fuseblk", "zfs",
            ]
            .contains(&fields[split + 1])
        {
            continue;
        }
        let path = fields[4]
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\134", "\\");
        if !Path::new(&path).is_dir() {
            continue;
        }
        if let Some(id) = resolve(c, Path::new(&path))? {
            if !devices.insert(fields[2].to_owned()) {
                continue;
            }
            disks.push(json!({"id":id,"path":path,"name":if path=="/"{"System disk".into()}else{Path::new(&path).file_name().unwrap_or_default().to_string_lossy().into_owned()},"device":fields[split+2],"filesystem":fields[split+1],"capacity":fs2::total_space(&path).ok(),"available":fs2::available_space(&path).ok()}));
        }
    }
    Ok(json!({"indexes":indexes,"disks":disks}))
}
