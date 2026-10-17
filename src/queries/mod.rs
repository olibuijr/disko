use crate::{
    cli::{Cli, Command, Kind},
    index::*,
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::BinaryHeap,
    io::{self, Write},
    path::{Path, PathBuf},
};
mod disks;
mod largest;
mod metadata;
mod output;
mod sql;
mod status;
pub use disks::filesystems;
pub use largest::largest_under;
pub use metadata::{node, required};
use output::{print_rows, rows};
pub use status::status_value;

pub fn run(cli: &Cli) -> Result<()> {
    let c = open(&cli.db, false)?;
    c.execute_batch("BEGIN")?; // A single snapshot also keeps path reconstruction consistent during renames.
    match &cli.command {
        Command::Du {
            path,
            limit,
            apparent,
        } => {
            let path = absolute(path)?;
            let id = required(&c, &path)?;
            let col = if *apparent { "bytes" } else { "blocks" };
            rows(
                &c,
                &format!("SELECT id FROM nodes WHERE parent=?1 ORDER BY {col} DESC LIMIT ?2"),
                params![id, *limit as i64],
                *apparent,
                cli.json,
            )
        }
        Command::Largest {
            path,
            kind,
            limit,
            apparent,
        } => {
            let path = absolute(path)?;
            let id = required(&c, &path)?;
            let col = if *apparent { "bytes" } else { "blocks" };
            let kind_num = if matches!(kind, Kind::Files) { 0 } else { 1 };
            if id == 1 {
                rows(
                    &c,
                    &format!(
                        "SELECT id FROM nodes WHERE kind={kind_num} AND id!=1 ORDER BY {col} DESC LIMIT ?1"
                    ),
                    [*limit as i64],
                    *apparent,
                    cli.json,
                )
            } else {
                let ids = largest_under(&c, id, kind_num, *limit, *apparent)?;
                print_rows(&c, &ids, *apparent, cli.json)
            }
        }
        Command::Info { path } => {
            let path = absolute(path)?;
            let id = required(&c, &path)?;
            let v = node(&c, id)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Command::Find {
            text,
            contains,
            limit,
        } => {
            if *contains {
                rows(
                    &c,
                    "SELECT id FROM nodes WHERE instr(CAST(name AS TEXT),?1)>0 LIMIT ?2",
                    params![text, *limit as i64],
                    false,
                    cli.json,
                )
            } else {
                if text.is_empty() {
                    bail!("provide a nonempty filename prefix");
                }
                let lo = text.as_bytes();
                let mut hi = lo.to_vec();
                *hi.last_mut().unwrap() += 1;
                rows(
                    &c,
                    "SELECT id FROM nodes WHERE name>=?1 AND name<?2 ORDER BY name LIMIT ?3",
                    params![lo, hi, *limit as i64],
                    false,
                    cli.json,
                )
            }
        }
        Command::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&status_value(&c, &cli.db)?)?
            );
            Ok(())
        }
        Command::Sql { query } => sql::sql(&c, query),
        _ => unreachable!(),
    }
}
