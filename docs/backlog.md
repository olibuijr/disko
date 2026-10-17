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

Adversarial review, 2026-10-17 (0.3.0):

| Item | Status |
|------|--------|
| Live dashboard accepted the default password `disko` | Rotated on titan; installer now random; dashboard warns |
| Rescans rewrote every row and index (1.2 GiB for 400k unchanged entries) | Fixed: unchanged rows are not written |
| Page rebuilt every 3 s: lost focus, screen reader spam | Fixed |
| No cleanup suggestions, no growth view | Added |
| Busy daemon shown as not running | Fixed |
| Full startup rescan after every restart | Fixed: an interrupted scan resumes from its saved queue; a restart after a finished scan still walks everything, without writes |
| Index commits every 2 s and Btrfs copy-on-write: 845k-extent index, swap and I/O thrash on titan | Fixed: larger batches, NOCOW index directory, idle priority |
| Case-insensitive and substring search in the dashboard | Open: needs an index that would grow the database |
| Snapshot list with exclusive sizes | Open: needs Btrfs quotas |

Open:

- The treemap at the index root mixes disks; selecting one disk is clearer.
- `unindexed` is approximate on compressed Btrfs, where file block counts and on-disk use differ.
