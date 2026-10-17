use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn disko(db: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_disko"));
    c.arg("--db").arg(db);
    c
}

fn sql(db: &Path, query: &str) -> Vec<Value> {
    let out = disko(db).args(["sql", query]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn scan(db: &Path, root: &Path) {
    let out = disko(db).arg("scan").arg(root).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Every node with its subtree totals, independent of row IDs and walk order.
fn snapshot(db: &Path) -> Vec<String> {
    let mut rows: Vec<_> = sql(
        db,
        "SELECT CAST(name AS TEXT) AS n,kind,size,bytes,files,entries,
         (SELECT CAST(name AS TEXT) FROM nodes p WHERE p.id=nodes.parent) AS parent FROM nodes",
    )
    .iter()
    .map(Value::to_string)
    .collect();
    rows.sort();
    rows
}

#[test]
fn interrupted_scan_resumes_where_it_stopped() {
    let t = TempDir::new_in("/tmp").unwrap();
    let root = t.path().join("tree");
    let db = t.path().join("state/index.db");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    for i in 0..300 {
        let dir = root.join(format!("bulk/{i}"));
        fs::create_dir_all(&dir).unwrap();
        for j in 0..200 {
            fs::write(dir.join(j.to_string()), [1; 3]).unwrap();
        }
    }
    let mut child = disko(&db)
        .args(["watch", "--no-web", "--jobs", "1"])
        .arg(&root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    // The index may not exist yet, so a failing query just means "not yet".
    let scanning = |db: &Path| {
        disko(db)
            .args(["sql", "SELECT value FROM meta WHERE key='state'"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("\"scanning\""))
    };
    while !scanning(&db) {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "scan never started"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(child.wait().unwrap().success());

    let pending = sql(&db, "SELECT count(*) AS n FROM scan_queue")[0]["n"]
        .as_i64()
        .unwrap();
    assert!(pending > 0, "the scan finished before it was interrupted");

    scan(&db, &root);
    let last = &sql(
        &db,
        "SELECT reason,visited FROM scans ORDER BY id DESC LIMIT 1",
    )[0];
    assert_eq!(last["reason"], "resumed after restart");
    assert!(
        last["visited"].as_i64().unwrap() < 300 * 201,
        "resume rescanned everything: {last}"
    );
    assert_eq!(sql(&db, "SELECT count(*) AS n FROM scan_queue")[0]["n"], 0);

    let fresh = db.with_file_name("fresh.db");
    scan(&fresh, &root);
    assert_eq!(snapshot(&fresh), snapshot(&db));
}
