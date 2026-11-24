use std::process::Command;

fn disko(db: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_disko"))
        .env("DISKO_DB", db)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn agent_commands_report_health_schema_and_skill() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(data.join("a")).unwrap();
    std::fs::write(data.join("a/f"), vec![1u8; 8192]).unwrap();
    let db = tmp.path().join("state/index.db");
    assert!(
        disko(&db, &["scan", data.to_str().unwrap()])
            .status
            .success()
    );

    // No daemon runs, so the verdict is fail (exit 1) with a reason, as JSON.
    let out = disko(&db, &["--json", "health", data.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["level"], "fail");
    assert_eq!(v["path_indexed"], true);
    assert!(v["problems"][0].as_str().unwrap().contains("not running"));

    // Errors counted since the daemon started do not warn; the last scan's do.
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch("UPDATE meta SET value='500' WHERE key IN ('metadata_errors','watch_errors')")
        .unwrap();
    let health = || -> serde_json::Value {
        let out = disko(&db, &["--json", "health"]);
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let v = health();
    assert_eq!(v["warnings"], serde_json::json!([]), "{v}");
    assert_eq!(v["metadata_errors_since_start"], 500);
    assert_eq!(v["last_complete_scan"]["metadata_errors"], 0);
    assert_eq!(v["last_complete_scan"]["mode"], "deep");
    c.execute_batch(
        "UPDATE scans SET errors=60, watch_errors=2 WHERE id=(SELECT max(id) FROM scans)",
    )
    .unwrap();
    let warnings = health()["warnings"].to_string();
    assert!(
        warnings.contains("60 entries could not be read in the last scan"),
        "{warnings}"
    );
    assert!(
        warnings.contains("2 folders could not be watched in the last scan"),
        "{warnings}"
    );
    let text = String::from_utf8(disko(&db, &["health"]).stdout).unwrap();
    assert!(
        text.contains("last scan: deep, ") && text.contains("60 metadata errors, 2 watch errors"),
        "{text}"
    );

    let out = disko(&db, &["--json", "schema"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["tables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["table"] == "nodes")
    );

    let out = disko(&db, &["--json", "du", "/no/such/path"]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["error"].as_str().unwrap().contains("not indexed"));

    let skills = tmp.path().join("skills");
    assert!(
        disko(&db, &["skill", "--install", skills.to_str().unwrap()])
            .status
            .success()
    );
    let text = std::fs::read_to_string(skills.join("disko/SKILL.md")).unwrap();
    assert!(text.starts_with("---\nname: disko\n"));
}
