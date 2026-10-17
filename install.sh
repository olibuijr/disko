#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
if (( EUID != 0 )); then
  echo 'Run cargo build --release, then sudo ./install.sh YOUR_USERNAME' >&2
  exit 1
fi
reader="${1:-${SUDO_USER:-}}"
if [[ -z "$reader" || "$reader" == root ]]; then
  echo 'Pass the username who should be able to read the filesystem index.' >&2
  exit 1
fi
reader_group="$(id -gn -- "$reader")"
[[ "$reader_group" =~ ^[a-zA-Z0-9_-]+$ ]]
test -x target/release/disko
install -m 0755 target/release/disko /usr/local/bin/disko
install -d -m 0750 -o root -g "$reader_group" /var/lib/disko
# The index is a rewrite-heavy SQLite file; Btrfs copy-on-write fragments it into
# hundreds of thousands of extents. NOCOW only applies to files created after it
# is set, so an existing index is rewritten once (the service restarts below).
if chattr +C /var/lib/disko 2>/dev/null && [[ -f /var/lib/disko/index.db ]] \
  && ! lsattr /var/lib/disko/index.db | cut -d" " -f1 | grep -q C; then
  echo "Rewriting the index without copy-on-write…"
  systemctl stop disko.service 2>/dev/null || true
  for f in index.db index.db-wal; do
    [[ -f "/var/lib/disko/$f" ]] || continue
    cp -p --reflink=never "/var/lib/disko/$f" "/var/lib/disko/$f.nocow"
    mv -f "/var/lib/disko/$f.nocow" "/var/lib/disko/$f"
  done
  rm -f /var/lib/disko/index.db-shm
fi
if [[ ! -f /var/lib/disko/web-password ]]; then
  password="$(head -c 18 /dev/urandom | base64 | tr -d '/+=')"
  printf '%s\n' "$password" | /usr/local/bin/disko password
  echo "Dashboard password: $password  (change it with: sudo disko password)"
fi
sed "s/@READER_GROUP@/$reader_group/" disko.service > /etc/systemd/system/disko.service
systemctl daemon-reload
systemctl enable disko.service
systemctl restart disko.service
echo 'Disko is running at http://0.0.0.0:7331. Use: disko status; disko du /; disko largest'
