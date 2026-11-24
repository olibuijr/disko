use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn folder(files: usize) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    for i in 0..files {
        fs::write(data.join(i.to_string()), b"x").unwrap();
    }
    (dir, data)
}

fn job(path: &Path, id: i64) -> walk::Job {
    walk::Job {
        path: path.to_owned(),
        id,
        seen: 1,
        place: walk::Place::Regular,
        only: None,
    }
}

#[test]
fn a_stop_interrupts_a_listed_folder_and_keeps_it_queued() {
    let (dir, data) = folder(50);
    let c = open(&dir.path().join("t.db"), true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let mut idx = Index::new(c, vec![data.clone()], vec![], stop.clone(), 1).unwrap();
    let id = idx.ensure(&data).unwrap().unwrap();
    idx.resumable = true;
    idx.queue_push(&job(&data, id)).unwrap();
    let listed = walk::walk_dir(&stop, &idx.filter, &job(&data, id));
    assert_eq!(listed.children.len(), 50);
    // SIGTERM while the writer replays the folder's entries.
    stop.store(true, Ordering::Relaxed);
    let mut pool = pool::Pool::start(stop, idx.filter.clone(), 1, job(&data, id)).unwrap();
    let error = idx.absorb(&mut pool, listed).unwrap_err().to_string();
    assert!(error.contains("interrupted"), "{error}");
    let written: i64 = idx
        .c
        .query_row("SELECT count(*) FROM nodes WHERE parent=?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(written, 0);
    let queued: i64 = idx
        .c
        .query_row("SELECT count(*) FROM scan_queue WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(queued, 1, "the folder must be reread after a restart");
}

fn wal_after_drop(stopping: bool) -> u64 {
    let (dir, data) = folder(20);
    let db = dir.path().join("t.db");
    let stop = Arc::new(AtomicBool::new(false));
    let mut idx = Index::new(
        open(&db, true).unwrap(),
        vec![data.clone()],
        vec![],
        stop.clone(),
        1,
    )
    .unwrap();
    idx.sync(&data, true).unwrap();
    idx.commit().unwrap();
    stop.store(stopping, Ordering::Relaxed);
    drop(idx);
    fs::metadata(db.with_extension("db-wal")).map_or(0, |m| m.len())
}

#[test]
fn a_stop_skips_the_wal_checkpoint() {
    // Normal close folds the WAL back; a stop leaves it for the next start.
    assert_eq!(wal_after_drop(false), 0);
    assert!(wal_after_drop(true) > 0);
}
