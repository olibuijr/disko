use super::*;

/// A deep scan is due when none has completed within `interval` seconds
/// (or ever), so incremental scans never run on an index they cannot trust.
pub(super) fn deep_due(idx: &Index, interval: u64) -> Result<bool> {
    let last: Option<i64> = idx
        .c
        .query_row(
            "SELECT value FROM meta WHERE key='last_deep_scan'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            // Indexes from before 0.4 recorded only `last_scan`; their scans were deep.
            idx.c
                .query_row("SELECT value FROM meta WHERE key='last_scan'", [], |r| {
                    r.get::<_, String>(0)
                })
                .ok()
                .and_then(|v| v.parse().ok())
        });
    Ok(match last {
        Some(t) => interval == 0 || now() - t >= interval as i64,
        None => true,
    })
}
