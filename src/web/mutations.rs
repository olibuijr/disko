use super::*;
use crate::{
    deletion::Target,
    index::{now, open, path_of, resolve},
    queries::node,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub(super) struct Preview {
    target: Target,
    session: String,
    expires: i64,
}

pub(super) fn handle(
    app: &App,
    session: &auth::Session,
    route: &str,
    body: Value,
) -> Result<Reply> {
    if app.rescan.is_none() {
        bail!("deletion is disabled in read-only dashboard mode");
    }
    if route == "/api/delete/preview" {
        let id = body["id"].as_i64().context("missing entry ID")?;
        let c = open(&app.db, false)?;
        c.execute_batch("BEGIN")?;
        let info = node(&c, id)?;
        let path = path_of(&c, id)?;
        let coverage: String =
            c.query_row("SELECT value FROM meta WHERE key='coverage'", [], |r| {
                r.get(0)
            })?;
        let coverage: Value = serde_json::from_str(&coverage)?;
        if coverage["roots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|p| path == Path::new(p))
        {
            bail!("index roots cannot be deleted");
        }
        let target = Target::prepare(
            &path,
            info["device"].as_i64().unwrap() as u64,
            info["inode"].as_i64().unwrap() as u64,
        )?;
        let token = auth::random_token()?;
        let mut previews = app.previews.lock().unwrap();
        previews.retain(|_, p| p.expires > now());
        if previews.len() >= 128 {
            bail!("too many pending delete previews");
        }
        previews.insert(
            token.clone(),
            Preview {
                target,
                session: session.id.clone(),
                expires: now() + 120,
            },
        );
        return Ok(Reply::json(
            200,
            json!({"token":token,"entry":info,"permanent":true}),
        ));
    }
    let token = body["token"]
        .as_str()
        .context("preview token is required")?;
    let _guard = app
        .deleting
        .try_lock()
        .map_err(|_| anyhow::anyhow!("another deletion is running"))?;
    let preview = app
        .previews
        .lock()
        .unwrap()
        .remove(token)
        .context("preview expired; refresh and try again")?;
    if preview.expires < now() || preview.session != session.id {
        bail!("preview expired; refresh and try again");
    }
    let path = preview.target.path.clone();
    // Verify the selected inode still exists in the index before touching the filesystem.
    let c = open(&app.db, false)?;
    if resolve(&c, &path)?.is_none() {
        bail!("entry is no longer indexed");
    }
    preview
        .target
        .delete()
        .context("deletion did not complete; the directory may be partially deleted")?;
    eprintln!("dashboard deletion completed: {}", path.display());
    Ok(Reply::json(
        200,
        json!({"deleted":true,"path":path.to_string_lossy(),"index_update":"pending watcher update"}),
    ))
}
