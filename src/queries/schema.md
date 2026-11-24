nodes: one row per indexed path (the root "/" is id 1, parent 0)
  id       row id, stable while a path exists (a rename keeps it)
  parent   id of the containing folder; a negative parent marks a removed subtree
           waiting to be deleted, so join from parent>0 or walk up to id 1
  name     file name as a BLOB (byte-exact); CAST(name AS TEXT) to read it
  kind     0 file, 1 directory, 2 symlink, 3 special
  size, allocated   this entry's own logical and allocated bytes
  bytes, blocks     subtree totals: logical and allocated bytes (blocks is bytes, not 512-byte blocks)
  files, entries    subtree counts (entries includes the node itself)
  mtime, ctime      Unix seconds
  dev, ino, links, mode, uid, gid   stat fields; links>1 marks hardlinks
meta: key/value status (state, pid, heartbeat, last_scan, scan_mode, coverage, ...)
scans: scan history (started, finished, visited, errors, reason, state)
growth: daily (day, id, blocks) record of folders of 1 GiB or more, 60 days
samples, activity: bounded rate history and recent changes
Queries are read-only and print JSON Lines. Filter with kind and parent indexes;
substring matches on name scan every row.
