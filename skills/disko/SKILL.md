---
name: disko
description: Answer disk-space questions on a Linux host from the Disko index instead of walking the filesystem, and operate the Disko daemon. Use when asked what is using disk space, why a disk is full, which folders or files are largest, what grew this week, which build caches can be cleaned, where a file is, who owns the space, or whether Disko is healthy, up to date, scanning or stuck. Also use to rescan, install, upgrade or troubleshoot Disko, or to write read-only SQL against its index. Triggers - disk usage, disk full, du, largest files, space hog, cleanup, node_modules, target folder, cache size, disko status, disko health.
license: MIT
compatibility: Linux with Disko installed (the disko command on PATH, the index at /var/lib/disko readable by the user's group). Rescan and install need sudo.
metadata:
  project: https://github.com/olibuijr/disko
  website: https://disko.olibuijr.com
---

# Disko

Disko keeps every folder's size in a SQLite index and updates it from kernel file
events. Ask the index; do not run `du`, `find` or `ncdu` over trees it covers.
Every command takes `--json` (errors then print `{"error": "..."}` and exit 1).
`disko skill` prints this file for the installed version.

## 1. Check the index before trusting numbers

```sh
disko --json health [PATH]
```

Exit status 0 is `ok`, 2 is `warn`, 1 is `fail`. Read `problems` and `warnings`:

- `fail`, daemon not running or no heartbeat for 15 minutes: totals are frozen.
  Say so, then see section 5.
- `warn`, `state=scanning`: totals are being reconciled. Report results with that
  qualification ("about", "as of the last scan").
- `path_indexed=false`: the path is outside coverage or excluded. Fall back to
  `du -sh` on that path only, and say why.

## 2. Answer space questions

| Question | Command |
| --- | --- |
| What fills this folder | `disko --json du PATH -n 20` |
| Largest files anywhere or below a path | `disko --json largest [PATH] --kind files -n 20` |
| Largest folder trees | `disko --json largest [PATH] --kind dirs -n 20` |
| Build output and caches tools can recreate | `disko --json cleanup -n 20` |
| What grew in the last week | `disko --json grew --days 7` |
| One path's size, owner, times, inode | `disko info PATH` |
| Files by name | `disko --json find PREFIX` (`--contains` scans every row: slower) |
| Coverage, disks and capacity | `disko --json status` |

To find why a disk is full, start at the mount point with `du`, descend into the
largest child two or three times, and finish with `largest --kind files` on the
deepest folder that explains the space. `cleanup` lists the safest wins first.

Sizes are allocated bytes by default; add `--apparent` for logical size. Totals
count paths: hardlinks, Btrfs snapshots, reflinks and compression mean a total is
not a promise of space freed by deleting it.

## 3. Custom questions with SQL

```sh
disko schema            # tables and what each column means
disko sql 'SELECT ...'  # read-only; prints JSON Lines
```

`nodes.bytes` and `nodes.blocks` are subtree totals in bytes. Rows with a negative
`parent` are removed subtrees waiting to be deleted: filter `parent > 0`. Add a
`LIMIT`. Examples:

```sh
disko sql 'SELECT uid, sum(allocated) AS bytes FROM nodes WHERE kind=0 AND parent>0 GROUP BY uid ORDER BY bytes DESC LIMIT 10'
disko sql "SELECT id, CAST(name AS TEXT) AS name, blocks FROM nodes WHERE kind=1 AND parent>0 AND CAST(name AS TEXT)='node_modules' ORDER BY blocks DESC LIMIT 20"
```

Turn an id into a path with `disko info` on its parent chain, or look it up in the
`du` and `largest` output, which already print paths.

## 4. Deleting files

Only delete what the user asked you to delete, by path, after showing them the
size and the path. Prefer the dashboard's Delete, which checks the inode, mount
boundaries and symlinks. Never delete inside `/var/lib/disko`, an index root, a
mount point or a snapshot. The index follows deletions on its own; do not rescan.

## 5. Operate the daemon

```sh
systemctl status disko          # running?
journalctl -u disko -n 50       # what it did last
sudo disko rescan               # ask for a deep scan (same as the dashboard button)
sudo systemctl restart disko    # resumes an interrupted scan where it stopped
```

Do not delete the index to fix freshness: restart, or rescan. Scans are
incremental daily and deep weekly; a stalled daemon restarts itself after 15
minutes. The dashboard is at http://HOST:7331 and needs its password.

Install or upgrade (static x86_64 binary):

```sh
curl -LO https://github.com/olibuijr/disko/releases/latest/download/disko-x86_64-linux.tar.gz
tar xzf disko-x86_64-linux.tar.gz && cd disko-x86_64-linux && sudo ./install.sh "$USER"
```

A user index without root: `DISKO_DB=~/.local/state/disko/index.db disko watch ~/src --listen 127.0.0.1:7331`
(keep the database outside the indexed folders; set a password first with `disko password`).

## 6. Working on Disko itself

Source: https://github.com/olibuijr/disko. Before a pull request run `cargo fmt`,
`cargo clippy --all-targets -- -D warnings`, `cargo test` and
`python3 tools/check_structure.py` (application files stay under 200 lines). Keep
`VERSION`, `Cargo.toml`, the README badge and `CHANGELOG.md` in step. Destructive
tests use fixtures under `/tmp` only.
