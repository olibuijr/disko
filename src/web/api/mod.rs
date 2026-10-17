mod cache;
mod disks;
mod explorer;
mod history;
use crate::{
    index::*,
    queries::{cleanup, growth, largest_under, node, required, status_value},
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

pub(super) fn query(db: &Path, route: &str, params: &HashMap<String, String>) -> Result<Value> {
    let c = open(db, false)?;
    c.execute_batch("PRAGMA cache_size=-4096; BEGIN")?;
    match route {
        "/api/status" => {
            let mut v = status_value(&c, db)?;
            v["version"] = json!(env!("CARGO_PKG_VERSION"));
            Ok(v)
        }
        "/api/roots" => disks::roots(&c),
        "/api/tree" | "/api/info" | "/api/largest" | "/api/find" => {
            explorer::query(&c, route, params)
        }
        "/api/history" | "/api/activity" => history::query(&c, route),
        "/api/cleanup" => Ok(cache::cleanup_list(db, params.contains_key("refresh"))),
        "/api/growth" => {
            let days = params
                .get("days")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(7)
                .clamp(1, 60);
            growth(&c, days, 10)
        }
        _ => bail!("unknown API route"),
    }
}
