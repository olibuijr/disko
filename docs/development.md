# Development

Application source is split into focused modules, with a 200-line file limit:

| Directory | Responsibility |
| --- | --- |
| `src/index/` | Schema, paths, scan reconciliation, metadata updates and commits |
| `src/daemon/` | Configuration, event coalescing and watcher lifecycle |
| `src/daemon/fanotify/` | Filesystem marks, handle resolution and event decoding |
| `src/queries/` | CLI query dispatch, metadata, status, ranking and output |
| `src/web/` | HTTP serving, assets, authentication, routes and mutation previews |
| `src/web/api/` | Read-only status, tree, search, disk and history APIs |
| `src/deletion/` | Descriptor-relative deletion and filesystem boundary checks |
| `web/js/` | Small native ES modules for individual pages and components |
| `web/styles/` | Dashboard layout and component styles |

## Required checks

```sh
./build.sh --no-deps --test
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
python3 tests/web_api.py
python3 tools/check_structure.py
bash -n build.sh install.sh
for source in web/js/*.js; do node --check "$source"; done
```

CI runs these checks on Linux. The HTTP tests use the debug binary built by
`cargo test`. Set `DISKO_BIN` to exercise another binary.

All destructive tests use fixtures under `/tmp`. The Rust tests cover sparse
files, hardlinks, non-UTF8 names, symlinks, aggregate reconciliation, replacement,
read-only SQL, and live events with scheduled reconciliation disabled. HTTP tests
cover authentication, CSRF/origin checks, preview identity, root refusal, recursive
deletion and symlink-target preservation.

## Browser checks

With Chromium installed:

```sh
uv tool run --from playwright python tests/browser_smoke.py
```

The test checks login, navigation, treemap selection, desktop/mobile layout,
Cancel/Delete behavior on temporary files, logout and browser errors. Screenshots
are written to `/tmp`. It uses `/usr/bin/chromium`; adjust that test path for
another local installation. Browser tooling is a development dependency only.

## Privileged watcher check

```sh
sudo env DISKO_BIN="$PWD/target/debug/disko" python3 tests/fanotify_linux.py
```

This creates a sparse ext4 image under `/tmp`, mounts it below `/tmp`, checks
fanotify events and deletion there, and unmounts it on exit. It needs root, loop
mount support and e2fsprogs. It does not operate on an existing disk's files.
Btrfs is covered through a detached top-level mount, which this ext4 fixture
does not exercise; check it by running `disko watch` as root on a scratch tree
below a Btrfs subvolume and confirming `watch_backend` and zero `watch_errors`.

Mount behaviour (mount and unmount while running, bind mounts, a Btrfs subvolume
watched through the top-level mark, and deletion refusing a nested subvolume):

```sh
sudo env DISKO_BIN="$PWD/target/release/disko" python3 tests/mounts_linux.py
```

## Releases

Update `Cargo.toml`, `VERSION`, `CHANGELOG.md` and the README version badge together.
Run the required checks, commit, tag `vVERSION`, and push the branch and tag.
The release binary embeds the UI, so rebuild after changing any asset. Runtime
data and password files are excluded from Git. Commit `Cargo.lock`.
