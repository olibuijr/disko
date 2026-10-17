# CLI and API reference

Global options: `--db PATH` (or `DISKO_DB`) and `--json`.

| Command | Purpose |
| --- | --- |
| `scan ROOT... [--exclude PATH]` | Reconcile once |
| `watch ROOT...` | Reconcile, watch and serve the dashboard |
| `watch --no-web` | Index and watch without HTTP |
| `watch --listen ADDRESS:PORT` | Choose dashboard listener; default `0.0.0.0:7331` |
| `watch --password-file PATH` | Choose a password hash file |
| `watch --reconcile-secs N` | Periodic reconciliation; default 86400 (daily), zero disables |
| `watch --debounce-ms N` | Event coalescing window; default 250, maximum 2000 |
| `serve [--listen ADDRESS:PORT]` | Read-only dashboard; default `127.0.0.1:7331` |
| `password [--password-file PATH]` | Hash a password read from stdin |
| `du PATH [-n N] [--apparent]` | Largest immediate children |
| `largest PATH [--kind files\|dirs] [-n N] [--apparent]` | Largest descendants |
| `info PATH` | Metadata and stored subtree totals |
| `find PREFIX [-n N] [--contains]` | Prefix or substring filename search |
| `status` | Coverage, progress and health |
| `sql QUERY` | Read-only SQLite query; JSON Lines output |

Password files default to `web-password` next to the database. Keep database
storage outside indexed roots. An existing index's coverage cannot be changed in
place; use another database. Paths in storage remain byte-exact; JSON includes
`path_bytes` when a path cannot be represented as UTF-8. Timestamps are Unix seconds.

## SQL

`nodes` columns: `id`, `parent`, `name` (BLOB), `kind` (file=0, directory=1,
symlink=2, special=3), `size`, `allocated`, `bytes`, `blocks`, `files`, `entries`,
`mtime`, `ctime`, `dev`, `ino`, `links`, `mode`, `uid`, `gid`, `seen`.

`size` and `allocated` are own-node bytes. `bytes` and `blocks` are subtree totals.
Directory own logical size is zero; allocated directory blocks are included.
`entries` includes the node itself. `meta`, `scans`, `samples` and `activity` hold
status and bounded history.

```sh
disko sql 'SELECT uid,count(*) AS files,sum(size) AS bytes FROM nodes WHERE kind=0 GROUP BY uid ORDER BY bytes DESC'
disko sql 'SELECT id,CAST(name AS TEXT) AS name,size,allocated FROM nodes WHERE kind=0 AND size>1073741824 AND allocated<size/10 ORDER BY size DESC LIMIT 20'
disko sql 'SELECT dev,ino,count(*) AS paths,max(size) AS bytes FROM nodes WHERE kind=0 AND links>1 GROUP BY dev,ino HAVING paths>1 ORDER BY bytes DESC LIMIT 20'
```

## HTTP API

`POST /api/login` accepts `{"password":"..."}` and sets the session cookie.
`GET /api/session` returns authentication state and, when signed in, a CSRF token.
Supply the cookie for all API requests and `X-Disko-Token` for POST mutations.
JSON request bodies are limited to 4 KiB. The UI is the reference client.

| Endpoint | Result |
| --- | --- |
| `GET /api/status` | Version, coverage, heartbeat, totals and scan progress |
| `GET /api/roots` | Configured index roots and discovered mounted disks |
| `GET /api/tree?id=ID&metric=allocated&limit=200` | Node, immediate children and ancestors |
| `GET /api/info?id=ID` | Cached node metadata; `path` can replace `id` |
| `GET /api/largest?id=ID&kind=files&metric=allocated&limit=100` | Largest nodes |
| `GET /api/find?q=PREFIX&limit=100` | Filename-prefix matches |
| `GET /api/history` | Scan history and rate samples |
| `GET /api/activity` | Recent coalesced updates |
| `POST /api/rescan` | Queue reconciliation |
| `POST /api/delete/preview` with `{"id":ID}` | Check eligibility and issue a preview token |
| `POST /api/delete` with `{"token":"..."}` | Delete the previewed entry |
| `POST /api/logout` | Invalidate the session |

`metric=apparent` selects logical size. List limits are bounded at 500.
Mutations are unavailable in `serve` mode. Authentication errors use 401, origin/
CSRF failures use 403, and invalid operations use 400 with a JSON `error` string.
