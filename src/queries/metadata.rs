use super::*;

pub fn required(c: &Connection, p: &Path) -> Result<i64> {
    resolve(c, p)?.with_context(|| {
        format!(
            "{} is not indexed; check `disko status` for coverage and scan progress",
            p.display()
        )
    })
}

pub fn node(c: &Connection, id: i64) -> Result<Value> {
    let mut v=c.query_row("SELECT kind,size,allocated,bytes,blocks,files,entries,mtime,ctime,dev,ino,links,mode,uid,gid FROM nodes WHERE id=?1",[id],|r|Ok(json!({
        "id":id,"kind":match r.get::<_,i64>(0)?{0=>"file",1=>"directory",2=>"symlink",_=>"special"},
        "size":r.get::<_,i64>(1)?,"allocated":r.get::<_,i64>(2)?,"apparent_bytes":r.get::<_,i64>(3)?,"allocated_bytes":r.get::<_,i64>(4)?,
        "files":r.get::<_,i64>(5)?,"entries":r.get::<_,i64>(6)?,"mtime":r.get::<_,i64>(7)?,"ctime":r.get::<_,i64>(8)?,
        "device":r.get::<_,i64>(9)?,"inode":r.get::<_,i64>(10)?,"links":r.get::<_,i64>(11)?,"mode":r.get::<_,i64>(12)?,"uid":r.get::<_,i64>(13)?,"gid":r.get::<_,i64>(14)?
    })))?;
    let path = path_of(c, id)?;
    v["path"] = json!(path.to_string_lossy());
    // Preserve non-UTF8 names without changing identity in the database.
    use std::os::unix::ffi::OsStrExt;
    if path.to_str().is_none() {
        v["path_bytes"] = json!(path.as_os_str().as_bytes());
    }
    Ok(v)
}
