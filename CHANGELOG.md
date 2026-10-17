# Changelog

All notable changes to Disko are recorded here. Versions follow Semantic Versioning.

## [0.3.0] - 2026-10-17

### Added

- Cleanup page and `disko cleanup`: the largest build-output, package and cache
  folders (Cargo `target/` and anything with `CACHEDIR.TAG`, `node_modules`,
  Python environments and caches, and more), outermost first, with delete.
  On a busy disk the query can take minutes, so the dashboard computes it in
  the background and serves it from memory.
- "Grew this week" on the overview and `disko grew`: a daily record of folders
  of 1 GiB or more, reported as the deepest folder that explains the growth.
- The dashboard warns while the password is still the old default `disko`.
- An interrupted full scan resumes after a restart instead of starting over. Its
  pending folders are kept in `scan_queue`, written in the same transactions as
  the entries, so the saved frontier is exact.

### Changed

- Rescans write only new and changed entries. Pruning compares each folder's
  indexed names with what was read, replacing per-row `seen` stamps. A rescan
  of `/usr` (400k entries) went from 201 s and 1,193 MiB written to 6.6 s and
  nothing written.
- The installer generates a random dashboard password instead of `disko`.
- Directory walkers default to at most 4; the single writer is the limit.
- The kernel fanotify queue is unbounded where permitted, so bursts do not
  overflow into a full rescan.
- Scans commit every 15 s or 32k changes instead of every 2 s or 4k, and fold
  the WAL back every 64 MiB instead of 16 MiB, so hot pages are written once
  per batch. On titan disko went from ~130 write IOPS to under one.
- The service yields to everything else: idle CPU and I/O scheduling, `Nice=19`,
  and first in line for the OOM killer.
- The installer marks `/var/lib/disko` no-copy-on-write and rewrites an existing
  index once. On a Btrfs root the 4 GiB index had fragmented into 845,085
  extents; rewritten it has 1,604.

### Fixed

- Dashboard refreshes no longer drop keyboard focus, re-render unchanged data,
  or re-announce the whole page to screen readers every three seconds.
- A daemon busy in a slow disk sync showed as "not running"; liveness now comes
  from the process, and a late heartbeat reads as busy.
- One unreadable entry (a file vanishing mid-scan) no longer raises a warning;
  the banner starts at 50.
- The overview fetches scan history every 30 seconds instead of every 3.

## [0.2.0] - 2026-10-17

### Added

- Per-filesystem space accounting in `status`, `/api/roots` and the dashboard:
  used, indexed, and space no indexed path accounts for (mostly snapshots).
- Mounts and unmounts while running are picked up within two seconds: new
  filesystems are marked for fanotify and changed mountpoints are resynced.
- Exclusions can change on an existing index; newly excluded trees are pruned.
- Full scans wait while less than 1 GiB is free for the index.
- `tests/mounts_linux.py`: root-only mount, bind, Btrfs subvolume and deletion
  boundary checks on loop images.

### Fixed

- Disko held a descriptor on every marked mountpoint, so `umount` failed while it ran.
- Btrfs subvolumes appeared as separate disks, summing capacity several times.
- Overlay and relocated bind mounts were counted twice.
- A full pending-event queue forced a rescan of every root; entries now merge
  into recursive parent resyncs.
- Deletion could enter a nested Btrfs subvolume and could stop half way at the
  depth limit; the whole tree is now checked before anything is removed.
- Startup scans recorded a stale reason from the previous run.
- Directory removal scanned every watched path and cleared the parent cache.

### Changed

- Reconciliation runs daily instead of every six hours.
- Dashboard reworked for legibility: proportional type with monospace only for
  paths and figures, plain-language labels, page titles that match navigation,
  and no decorative chrome.
- Overview leads with a per-disk space ledger (in the index, not in the index,
  reserved, free) that opens the disk tree; scans show progress, speed and time
  left; recent changes show names with start-truncated folders and relative times.
- Explorer shows size and last modified instead of type, file count and uid;
  history shows plain reasons and no longer reports zero entries for scans that
  did not finish; delete confirmations name the action.

## [0.1.2] - 2026-10-17

### Fixed

- Watch Btrfs subvolumes with one filesystem-wide fanotify mark (through a detached
  read-only top-level mount) instead of one inotify watch per directory. On a
  16.5M-entry machine this frees the ~1 GiB the watches pinned, covers the 43k
  directories past the inotify limit, and stops starving other root services of
  inotify watches.
- A stop signal during a scan now exits 0 for `disko watch`, so systemd no longer
  records restarts as failures. One-shot `disko scan` still fails when interrupted.
- The scan receiver no longer waits forever if workers stop; workers stop taking
  queued directories as soon as a sweep is abandoned.
- Cap the WAL at 64 MiB and truncate it after each full scan; it had grown to 1 GiB.

### Changed

- Parallel scans walk depth-first and start workers on demand, so small
  event-driven sweeps no longer spawn a full crew and the backlog stays small.
- Reuse walker metadata for watch decisions instead of a second `stat` per directory.
- The service backs off restarts up to 30 minutes instead of rescanning every 5 s
  when it keeps failing, for example on a full disk.

## [0.1.1] - 2026-10-16

### Fixed

- Refresh disk capacity during dashboard polling and refresh disk-picker labels when opened.

### Changed

- Pin CI to Ubuntu 24.04 and a Node 24 checkout action to remove runner deprecation warnings.

## [0.1.0] - 2026-10-16

### Added

- Rust filesystem metadata daemon with persistent SQLite WAL storage.
- Compact parent/name indexing, stored directory totals, batched ancestor updates,
  indexed largest-file queries, and best-first subtree queries.
- Live fanotify filesystem marks with inotify fallback, debouncing, bounded event queues, overflow recovery,
  startup reconciliation and configurable periodic reconciliation.
- CLI commands: `du`, `largest`, `info`, `find`, `status`, `sql`, `scan`, `watch` and `serve`.
- Embedded authenticated LAN dashboard with progress, health, space explorer, search,
  scan history, indexing-rate history and recent changes.
- Password login, session CSRF protection and previewed file/folder deletion.
- Raw filename preservation, sparse-file allocation reporting, hardlink identity,
  ownership and timestamps. Symlinks are recorded without following them.
- Linux dependency bootstrap and release build through `build.sh`.
- Hardened systemd service, group-readable index, MIT license, CI and agent skill.
- Integration tests and a reproducible metadata-only performance benchmark.

### Accounting notes

- Space totals count paths. Hardlinks, Btrfs snapshots, reflinks and compression
  require filesystem-specific analysis to estimate reclaimable space.
- Virtual filesystems, `.snapshots` directories and index storage are excluded.

[0.1.0]: https://github.com/olibuijr/disko/releases/tag/v0.1.0

[0.1.1]: https://github.com/olibuijr/disko/releases/tag/v0.1.1

[0.1.2]: https://github.com/olibuijr/disko/releases/tag/v0.1.2

[0.2.0]: https://github.com/olibuijr/disko/releases/tag/v0.2.0

[0.3.0]: https://github.com/olibuijr/disko/releases/tag/v0.3.0
