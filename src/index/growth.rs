use super::*;

/// Folders at least this large are recorded in the daily size history.
const GROWTH_MIN_BYTES: i64 = 1 << 30;
/// Days of history kept.
const GROWTH_DAYS: i64 = 60;

impl Index {
    /// Record today's size of every large folder, once per day, so the
    /// dashboard can show what grew. Uses the partial `dir_blocks` index, so
    /// it reads only the few thousand rows above the threshold.
    pub fn record_growth(&mut self) -> Result<()> {
        let today = now() / 86400;
        let last: i64 = self
            .c
            .query_row("SELECT value FROM meta WHERE key='growth_day'", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if last == today {
            return Ok(());
        }
        self.commit()?;
        self.c.execute(
            "INSERT OR REPLACE INTO growth SELECT ?1,id,blocks FROM nodes WHERE kind=1 AND blocks>=?2",
            params![today, GROWTH_MIN_BYTES],
        )?;
        self.c
            .execute("DELETE FROM growth WHERE day<?1", [today - GROWTH_DAYS])?;
        set_meta(&self.c, "growth_day", today)?;
        self.commit()
    }
}
