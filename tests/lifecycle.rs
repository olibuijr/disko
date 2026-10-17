use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn run(db: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(db)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn scan(db: &Path, root: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(db)
        .arg("scan")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "watcher failed to converge"
        );
        thread::sleep(Duration::from_millis(50));
    }
}
struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn setup() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let t = TempDir::new_in("/tmp").unwrap();
    let root = t.path().join("tree");
    fs::create_dir(&root).unwrap();
    let state = t.path().join("state");
    fs::create_dir(&state).unwrap();
    let db = state.join("index.db");
    (t, root, db)
}

#[test]
fn scan_sparse_links_non_utf8_and_reconcile() {
    use std::os::unix::ffi::OsStringExt;
    let (_t, root, db) = setup();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("nested/a"), vec![1; 8192]).unwrap();
    fs::hard_link(root.join("nested/a"), root.join("hardlink")).unwrap();
    fs::File::create(root.join("sparse"))
        .unwrap()
        .set_len(1 << 30)
        .unwrap();
    symlink("nested", root.join("loop")).unwrap();
    fs::create_dir(root.join(".snapshots")).unwrap();
    fs::write(root.join(".snapshots/excluded"), [1; 99]).unwrap();
    fs::write(
        root.join(std::ffi::OsString::from_vec(vec![b'x', 255])),
        [1; 17],
    )
    .unwrap();
    scan(&db, &root);
    let info = run(&db, &["info", root.to_str().unwrap()]);
    assert_eq!(info["files"], 4); // Count hardlinks once per pathname; symlinks are not followed.
    assert_eq!(
        info["apparent_bytes"].as_i64().unwrap(),
        (1 << 30) + 8192 * 2 + 17 + 6
    );
    let largest = run(
        &db,
        &["largest", root.to_str().unwrap(), "--apparent", "-n", "1"],
    );
    assert_eq!(largest[0]["path"], root.join("sparse").to_str().unwrap());
    let odd = run(&db, &["find", "x"]);
    assert!(
        odd.as_array()
            .unwrap()
            .iter()
            .any(|v| v.get("path_bytes").is_some())
    );
    fs::remove_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("hardlink"), [1; 31]).unwrap();
    scan(&db, &root);
    let info = run(&db, &["info", root.to_str().unwrap()]);
    assert_eq!(info["files"], 3);
    assert_eq!(
        info["apparent_bytes"].as_i64().unwrap(),
        (1 << 30) + 31 + 17 + 6
    );
    let c = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn watcher_create_resize_rename_delete_and_hardlinks() {
    let (_t, root, db) = setup();
    fs::write(root.join("a"), [1; 10]).unwrap();
    fs::hard_link(root.join("a"), root.join("b")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(&db)
        .arg("watch")
        .arg("--no-web")
        .arg(&root)
        .arg("--reconcile-secs")
        .arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _daemon = Daemon(child);
    wait(|| {
        db.exists()
            && Command::new(env!("CARGO_BIN_EXE_disko"))
                .arg("--db")
                .arg(&db)
                .arg("status")
                .output()
                .is_ok_and(|o| {
                    o.status.success() && String::from_utf8_lossy(&o.stdout).contains("watching")
                })
    });
    fs::write(root.join("a"), [1; 4097]).unwrap();
    wait(|| run(&db, &["info", root.to_str().unwrap()])["apparent_bytes"] == 8194);
    fs::create_dir_all(root.join("new/deep")).unwrap();
    fs::write(root.join("new/deep/c"), [1; 1000]).unwrap();
    wait(|| run(&db, &["info", root.to_str().unwrap()])["files"] == 3);
    fs::rename(root.join("new"), root.join("moved")).unwrap();
    wait(|| {
        run(&db, &["find", "c"])
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["path"] == root.join("moved/deep/c").to_str().unwrap())
    });
    fs::remove_dir_all(root.join("moved")).unwrap();
    fs::remove_file(root.join("b")).unwrap();
    wait(|| run(&db, &["info", root.to_str().unwrap()])["files"] == 1);
    let info = run(&db, &["info", root.join("a").to_str().unwrap()]);
    assert_eq!(
        info["allocated"],
        fs::metadata(root.join("a")).unwrap().blocks() * 512
    );
    assert_eq!(info["apparent_bytes"], 4097);
}

#[test]
fn directory_replaced_by_file_and_sql_cannot_write() {
    let (_t, root, db) = setup();
    fs::create_dir(root.join("a")).unwrap();
    fs::write(root.join("a/old"), [1; 100]).unwrap();
    scan(&db, &root);
    fs::remove_dir_all(root.join("a")).unwrap();
    fs::write(root.join("a"), [1; 500]).unwrap();
    scan(&db, &root);
    let info = run(&db, &["info", root.to_str().unwrap()]);
    assert_eq!(info["files"], 1);
    assert_eq!(info["apparent_bytes"], 500);
    let out = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(&db)
        .args(["sql", "DELETE FROM nodes"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}

fn scan_jobs(db: &Path, root: &Path, jobs: u32) {
    let out = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(db)
        .arg("scan")
        .arg(root)
        .arg("--jobs")
        .arg(jobs.to_string())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Every node with its subtree totals, independent of row IDs and walk order.
fn snapshot(db: &Path) -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(db)
        .args([
            "sql",
            "SELECT CAST(name AS TEXT) AS n,kind,size,bytes,files,entries,
             (SELECT CAST(name AS TEXT) FROM nodes p WHERE p.id=nodes.parent) AS parent
             FROM nodes",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let mut rows: Vec<_> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert!(rows.len() > 300, "{} rows", rows.len());
    rows.sort();
    rows
}

#[test]
fn parallel_and_serial_walks_agree_including_pruning() {
    let (_t, root, db) = setup();
    for i in 0..12 {
        for j in 0..6 {
            let dir = root.join(format!("d{i}/e{j}"));
            fs::create_dir_all(&dir).unwrap();
            for k in 0..4 {
                fs::write(dir.join(format!("f{k}")), vec![1; i * 100 + j * 10 + k]).unwrap();
            }
        }
    }
    let mut deep = root.join("deep");
    for level in 0..40 {
        deep = deep.join(format!("l{level}"));
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("leaf"), [1; 77]).unwrap();
    let serial = db.with_file_name("serial.db");
    scan_jobs(&serial, &root, 1);
    scan_jobs(&db, &root, 8);
    assert_eq!(snapshot(&serial), snapshot(&db));

    fs::remove_dir_all(root.join("d3")).unwrap();
    fs::remove_dir_all(root.join("deep/l0/l1/l2")).unwrap();
    fs::write(root.join("d5/e2/f0"), [1; 9999]).unwrap();
    fs::create_dir_all(root.join("d7/new/inner")).unwrap();
    fs::write(root.join("d7/new/inner/x"), [1; 5]).unwrap();
    scan_jobs(&db, &root, 8);
    let fresh = db.with_file_name("fresh.db");
    scan_jobs(&fresh, &root, 1);
    assert_eq!(snapshot(&fresh), snapshot(&db));
    let info = run(&db, &["info", root.to_str().unwrap()]);
    assert_eq!(info["files"], 11 * 6 * 4 + 1);
}

fn sigterm_exits_cleanly(child: &mut Child, db: &Path) {
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "daemon did not stop"
        );
        thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "SIGTERM exit status: {status}");
    assert_eq!(run(db, &["status"])["state"], "stopped");
}

fn spawn_watch(db: &Path, root: &Path, jobs: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(db)
        .arg("watch")
        .arg("--no-web")
        .arg(root)
        .args(["--jobs", jobs])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn state(db: &Path) -> String {
    Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(db)
        .arg("status")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[test]
fn watch_stops_cleanly_on_sigterm_while_idle_and_mid_scan() {
    let (_t, root, db) = setup();
    fs::write(root.join("a"), [1; 10]).unwrap();
    let mut child = spawn_watch(&db, &root, "0");
    wait(|| db.exists() && state(&db).contains("\"watching\""));
    sigterm_exits_cleanly(&mut child, &db);

    // Enough entries that a single-worker walk is still running when signalled.
    for i in 0..400 {
        let dir = root.join(format!("bulk/{i}"));
        fs::create_dir_all(&dir).unwrap();
        for j in 0..250 {
            fs::File::create(dir.join(j.to_string())).unwrap();
        }
    }
    let fresh = db.with_file_name("mid-scan.db");
    let mut child = spawn_watch(&fresh, &root, "1");
    wait(|| fresh.exists() && state(&fresh).contains("\"scanning\""));
    sigterm_exits_cleanly(&mut child, &fresh);
}

#[test]
fn exclusions_change_in_place_without_a_new_index() {
    let (_t, root, db) = setup();
    fs::create_dir_all(root.join("keep")).unwrap();
    fs::write(root.join("keep/a"), [1; 100]).unwrap();
    fs::create_dir_all(root.join("cache/deep")).unwrap();
    fs::write(root.join("cache/deep/b"), [1; 900]).unwrap();
    scan(&db, &root);
    assert_eq!(
        run(&db, &["info", root.to_str().unwrap()])["apparent_bytes"],
        1000
    );
    let excluded = root.join("cache");
    let out = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(&db)
        .arg("scan")
        .arg(&root)
        .arg("--exclude")
        .arg(&excluded)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let info = run(&db, &["info", root.to_str().unwrap()]);
    assert_eq!(info["apparent_bytes"], 100);
    assert_eq!(info["files"], 1);
    scan(&db, &root); // Dropping the exclusion indexes the tree again.
    assert_eq!(
        run(&db, &["info", root.to_str().unwrap()])["apparent_bytes"],
        1000
    );
    let other = root.join("keep");
    let out = Command::new(env!("CARGO_BIN_EXE_disko"))
        .arg("--db")
        .arg(&db)
        .arg("scan")
        .arg(&other)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "changing roots must still be refused"
    );
}
