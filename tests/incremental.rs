//! Incremental scans, single-row renames and batched removal.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn disko(db: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_disko"));
    c.arg("--db").arg(db).arg("--json");
    c
}
fn ok(c: &mut Command) -> Vec<u8> {
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn sql(db: &Path, q: &str) -> Vec<Value> {
    String::from_utf8(ok(disko(db).args(["sql", q])))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn info(db: &Path, p: &Path) -> Option<Value> {
    let out = disko(db).arg("info").arg(p).output().unwrap();
    out.status
        .success()
        .then(|| serde_json::from_slice(&out.stdout).unwrap())
}
fn last_visited(db: &Path) -> i64 {
    sql(db, "SELECT visited FROM scans ORDER BY id DESC LIMIT 1")[0]["visited"]
        .as_i64()
        .unwrap()
}
fn tree() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let t = TempDir::new_in("/tmp").unwrap();
    let root = t.path().join("tree");
    for i in 0..20 {
        let d = root.join(format!("d{i}/sub"));
        fs::create_dir_all(&d).unwrap();
        for j in 0..50 {
            fs::write(d.join(format!("f{j}")), [1; 10]).unwrap();
        }
    }
    let db = t.path().join("state/index.db");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    (t, root, db)
}

#[test]
fn incremental_scan_rereads_only_changed_folders() {
    let (_t, root, db) = tree();
    // Timestamps have one-second resolution: folders changed in the second a
    // scan started are never trusted, so let the clock move on between steps.
    let tick = || thread::sleep(Duration::from_millis(1100));
    tick();
    ok(disko(&db).arg("scan").arg(&root));
    let deep = last_visited(&db);
    tick();
    // Unchanged tree: only folders are checked, never the 1,000 files.
    ok(disko(&db).args(["scan", "--incremental"]).arg(&root));
    assert!(last_visited(&db) < 100, "{} of {deep}", last_visited(&db));
    // Added and removed entries change the folder's mtime and are found.
    fs::write(root.join("d3/sub/new"), [1; 7]).unwrap();
    fs::remove_file(root.join("d4/sub/f0")).unwrap();
    fs::remove_dir_all(root.join("d5")).unwrap();
    ok(disko(&db).args(["scan", "--incremental"]).arg(&root));
    assert!(last_visited(&db) < 200, "{}", last_visited(&db));
    assert_eq!(info(&db, &root.join("d3/sub/new")).unwrap()["size"], 7);
    assert!(info(&db, &root.join("d4/sub/f0")).is_none());
    assert!(info(&db, &root.join("d5")).is_none());
    assert_eq!(info(&db, &root).unwrap()["files"], 19 * 50);
    assert_eq!(
        sql(&db, "SELECT count(*) AS n FROM nodes WHERE parent<0")[0]["n"],
        0
    );
}

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "watcher failed to converge"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn watched_rename_moves_the_subtree_and_removal_is_collected() {
    let (_t, root, db) = tree();
    let _d = Daemon(
        Command::new(env!("CARGO_BIN_EXE_disko"))
            .arg("--db")
            .arg(&db)
            .args(["watch", "--no-web"])
            .arg(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait(|| {
        info(&db, &root.join("d19/sub/f49")).is_some()
            && sql(&db, "SELECT value FROM meta WHERE key='state'")[0]["value"] == "watching"
    });
    let id = info(&db, &root.join("d1")).unwrap()["id"].clone();
    fs::rename(root.join("d1"), root.join("moved")).unwrap();
    wait(|| {
        info(&db, &root.join("moved/sub/f3")).is_some() && info(&db, &root.join("d1")).is_none()
    });
    // With fanotify (root) the folder keeps its row: it moved rather than being
    // deleted and rescanned. Unprivileged runs use inotify, which rebuilds it.
    let backend = sql(&db, "SELECT value FROM meta WHERE key='watch_backend'");
    if backend[0]["value"]
        .as_str()
        .unwrap_or("")
        .contains("fanotify")
    {
        assert_eq!(info(&db, &root.join("moved")).unwrap()["id"], id);
    }
    assert_eq!(info(&db, &root).unwrap()["files"], 1000);
    fs::remove_dir_all(root.join("d2")).unwrap();
    wait(|| info(&db, &root).unwrap()["files"] == 950);
    // Detached rows are deleted in the background.
    wait(|| {
        sql(
            &db,
            "SELECT (SELECT count(*) FROM nodes)-(SELECT entries FROM nodes WHERE id=1) AS n",
        )[0]["n"]
            == 0
    });
}
