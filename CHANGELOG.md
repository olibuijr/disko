# Changelog

All notable changes to Disko are recorded here. Versions follow Semantic Versioning.

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
