use super::*;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn open(path: &Path, write: bool) -> Result<Connection> {
    if write && let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let flags = if write {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
    } else {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let c = Connection::open_with_flags(path, flags).with_context(|| {
        format!(
            "open {}; start `disko watch` to create the index",
            path.display()
        )
    })?;
    c.busy_timeout(Duration::from_secs(30))?;
    c.execute_batch("PRAGMA cache_size=-32768; PRAGMA temp_store=MEMORY;")?;
    c.set_prepared_statement_cache_capacity(64);
    if write {
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=16384; PRAGMA journal_size_limit=134217728;
        CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS nodes(
          id INTEGER PRIMARY KEY, parent INTEGER NOT NULL, name BLOB NOT NULL,
          kind INTEGER NOT NULL, size INTEGER NOT NULL, allocated INTEGER NOT NULL,
          bytes INTEGER NOT NULL, blocks INTEGER NOT NULL, files INTEGER NOT NULL, entries INTEGER NOT NULL,
          mtime INTEGER NOT NULL, ctime INTEGER NOT NULL, dev INTEGER NOT NULL, ino INTEGER NOT NULL,
          links INTEGER NOT NULL, mode INTEGER NOT NULL, uid INTEGER NOT NULL, gid INTEGER NOT NULL,
          seen INTEGER NOT NULL, UNIQUE(parent,name));
        CREATE INDEX IF NOT EXISTS file_bytes ON nodes(bytes DESC,id) WHERE kind=0;
        CREATE INDEX IF NOT EXISTS file_blocks ON nodes(blocks DESC,id) WHERE kind=0;
        CREATE INDEX IF NOT EXISTS dir_bytes ON nodes(bytes DESC,id) WHERE kind=1;
        CREATE INDEX IF NOT EXISTS dir_blocks ON nodes(blocks DESC,id) WHERE kind=1;
        CREATE INDEX IF NOT EXISTS hardlinks ON nodes(dev,ino) WHERE links>1 AND kind=0;
        CREATE INDEX IF NOT EXISTS names ON nodes(name);
        CREATE INDEX IF NOT EXISTS child_blocks ON nodes(parent,kind,blocks DESC);
        CREATE INDEX IF NOT EXISTS child_bytes ON nodes(parent,kind,bytes DESC);
        CREATE TABLE IF NOT EXISTS scans(id INTEGER PRIMARY KEY,started INTEGER NOT NULL,finished INTEGER,visited INTEGER NOT NULL DEFAULT 0,errors INTEGER NOT NULL DEFAULT 0,watch_errors INTEGER NOT NULL DEFAULT 0,reason TEXT NOT NULL,state TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS samples(ts INTEGER PRIMARY KEY,entries INTEGER NOT NULL,files INTEGER NOT NULL,bytes INTEGER NOT NULL,allocated INTEGER NOT NULL,visited INTEGER NOT NULL,watches INTEGER NOT NULL,state TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS growth(day INTEGER NOT NULL,id INTEGER NOT NULL,blocks INTEGER NOT NULL,PRIMARY KEY(day,id)) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS scan_queue(id INTEGER PRIMARY KEY,place INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS activity(id INTEGER PRIMARY KEY,ts INTEGER NOT NULL,path BLOB NOT NULL,kind TEXT NOT NULL);
        INSERT OR IGNORE INTO nodes VALUES(1,0,x'2f',1,0,0,0,0,0,1,0,0,0,0,1,0,0,0,0);
        INSERT OR IGNORE INTO meta VALUES('schema','1');")?;
    }
    let version: String = c.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
        r.get(0)
    })?;
    if version != "1" {
        bail!("unsupported index schema {version}");
    }
    Ok(c)
}

pub fn set_meta(c: &Connection, k: &str, v: impl ToString) -> Result<()> {
    c.execute(
        "INSERT INTO meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![k, v.to_string()],
    )?;
    Ok(())
}
