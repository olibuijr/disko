# Access and file deletion

## Network access

The installed daemon binds `0.0.0.0:7331`. The initial password is `disko`;
`install.sh` sets it only when the password file does not already exist. Use
`disko password` with a password on stdin, then restart, to change it.

The password file contains an Argon2id hash, uses mode 0600 and is replaced
atomically. The server keeps at most 64 sessions in memory. Session cookies are
HttpOnly and SameSite=Strict, expire after 12 hours, and are invalidated by restart.
Login attempts are limited to five per source IP per minute.

All metadata APIs require authentication. Mutation APIs also require a per-session
CSRF header. Host validation accepts localhost and numeric LAN/loopback addresses;
requests with a different Origin are refused. API responses do not enable CORS.
The UI uses text nodes for filenames and a restrictive Content Security Policy.

HTTP does not encrypt passwords or cookies on the network. Keep the service on a
trusted LAN. For encrypted remote use, bind it to localhost and use an SSH tunnel,
or configure an HTTPS reverse proxy. Named reverse-proxy hosts are not currently
accepted by the built-in Host/Origin validation; preserve a numeric upstream Host
and matching Origin or use the localhost tunnel. Do not forward port 7331 to the internet.

## Host access

The system service reads metadata as root. The named reader user's primary group
can query the database, including metadata for files they could not normally
list. The index and password hash stay in `/var/lib/disko`; no credentials,
databases or real scan history belong in the source repository.

The service uses a read-only system view with write access to its state directory
and supported data locations. An unprivileged daemon can only index/read/delete
what its user permissions allow. `serve` has no writer and disables deletion and
reconciliation requests.

## Deletion rules

The user opens a preview, then chooses Cancel or Delete. There is no typed
confirmation. A preview is single-use, session-bound and expires after two minutes.
Deletion is permanent. Stored size is not an estimate of exclusive reclaimable blocks.

The backend:

- accepts indexed IDs, resolves their stored raw paths, and refuses configured
  index roots, mount roots and `.snapshots`;
- permits only descendants of `/home`, `/mnt`, `/media`, `/tmp` and `/var/tmp`;
- opens every ancestor using directory descriptors and `O_NOFOLLOW`;
- checks device/inode identity at preview and deletion;
- traverses directories with descriptor-relative operations and refuses nested
  mount boundaries;
- unlinks symlinks instead of following their targets;
- serializes deletion requests and limits recursion depth.

An identity change aborts the operation. Directory operations are not atomic;
permissions, concurrent renames, new files or I/O failures can leave a folder
partially deleted. A concurrent writer can race a final name-based unlink; Unix
does not provide an atomic “unlink only this inode” operation. The descriptor
boundary prevents ancestor-symlink traversal but is not a transaction over the tree.

Tests that delete anything use disposable fixtures under `/tmp`, including a
loop-mounted ext4 image for privileged fanotify coverage. Never run destructive
smoke tests against a user's indexed data.
