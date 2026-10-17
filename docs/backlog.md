# Backlog

Review findings from 2026-10-17 and what happened to each.

| # | Item | Status |
|---|------|--------|
| 1 | Snapshot and other unindexed space was invisible (621 GiB on the system disk) | Fixed in 0.2.0: per-filesystem `used`, `indexed`, `unindexed` in `status`, `/api/roots` and the overview |
| 2 | A full pending-event queue during a long scan forced a rescan of every root | Fixed in 0.2.0: entries merge into recursive parent resyncs |
| 3 | Changing exclusions required a new index | Fixed in 0.2.0: exclusions change in place; roots still cannot |
| 4 | Dashboard runs as root over plain HTTP on all interfaces, with deletion | Kept as is by decision; password, same-origin and CSRF checks remain |
| 5 | Mounts added after startup were not watched; marked mounts could not be unmounted | Fixed in 0.2.0: mount table polling, per-event decoding without held descriptors |
| 6 | Overlay and relocated bind mounts were counted twice | Fixed in 0.2.0: recorded but not descended |
| 7 | A full disk crashed the daemon into rescan loops | Fixed in 0.2.0: scans wait below 1 GiB free; service restart backoff |
| 8 | Reconciliation every six hours while a scan took hours | Fixed in 0.2.0: daily by default |
| 9 | Directory removal scanned every watch and cleared the parent cache | Fixed in 0.2.0: ordered watch ranges, targeted cache invalidation |
| 10 | Deletion could enter nested Btrfs subvolumes and leave partial deletions past the depth limit | Fixed in 0.2.0: full preflight walk, subvolume boundary |
| 11 | No tests for the Btrfs path or pool shutdown | Fixed in 0.2.0: `tests/mounts_linux.py`, pool unit tests |
| — | Dashboard used Btrfs subvolumes as separate disks, summing capacity four times | Fixed in 0.2.0 |
| — | Web UI: unreadable terminal styling, jargon, misleading scan rate and history counts | Reworked in 0.2.0; see the changelog |

Open:

- The treemap at the index root mixes disks; selecting one disk is clearer.
- `unindexed` is approximate on compressed Btrfs, where file block counts and on-disk use differ.
