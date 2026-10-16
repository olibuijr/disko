---
name: disko
description: Query the Disko filesystem metadata index for disk usage, large files and directories, filename search, ownership, allocation and watcher health. Use when the user wants filesystem space analysis on a host running Disko, or asks to operate or develop this daemon.
---

# Disko

Use the existing metadata index before starting a filesystem walk.

## Query a running installation

1. Run `disko status` to inspect coverage, state, heartbeat, writer health and errors.
2. Use `disko --json du PATH` for immediate children and stored subtree totals.
3. Use `disko --json largest PATH --kind files` or `--kind dirs` for large entries.
4. Use `disko info PATH` for metadata, or `disko --json find PREFIX` for filename search.
5. Use `disko sql 'SELECT ...'` for read-only custom queries. The schema is in
   [docs/reference.md](docs/reference.md).

`DISKO_DB` or `--db PATH` selects another database. CLI reads do not require the
web password. The database directory must be readable by the caller.

## Interpret results

- `state=scanning` means totals are partial for a first scan and being reconciled
  for an existing index. Report that qualification with the result.
- Check `heartbeat_age_seconds`, `writer_alive`, `metadata_errors`, `watch_errors`
  and `watch_backend`. A stored total does not prove the watcher is healthy.
- Default space queries report allocated bytes. `--apparent` reports logical bytes.
- Totals count paths. Hardlinks, compression, reflinks, snapshots and excluded
  trees prevent treating these numbers as guaranteed reclaimable bytes.
- Do not run repeated `du` or `find` scans when the index covers the requested path.
  Filename substring searches and arbitrary SQL may scan many index rows.

## Dashboard

The installed service listens on port 7331. It provides Overview, Disk Tree,
Space explorer and History. Disk Tree box area represents stored space usage;
its dropdown selects an index root or discovered mounted disk. The HTTP API
requires password authentication; mutations also require a session CSRF token.
See [docs/security.md](docs/security.md) before changing exposure or permissions.

## File deletion

Do not delete user files unless their instruction authorizes that deletion.
The web UI previews an indexed item and uses a Cancel/Delete confirmation dialog.
The backend checks the session, CSRF token, preview expiry, inode, allowed path,
mount boundaries and symlink traversal. Prefer that flow over ad hoc shell deletion.
All destructive development tests must use fixtures below `/tmp`.

## Build and maintain

```sh
./build.sh --test
./build.sh --install
```

`--install` installs/restarts the systemd daemon. For diagnostics use
`systemctl status disko` and `journalctl -u disko`. A restart reconciles persisted
metadata with the filesystem. Do not delete the database to resolve routine
watcher or freshness problems.

When editing the project, preserve the 200-line application-source limit and
focused modules. Keep `VERSION`, `Cargo.toml`, the README badge and `CHANGELOG.md`
consistent. Run the checks in [docs/development.md](docs/development.md), update
relevant documentation, and keep credentials and runtime indexes out of Git.
