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
if [[ ! -f /var/lib/disko/web-password ]]; then
  printf '%s\n' disko | /usr/local/bin/disko password
fi
sed "s/@READER_GROUP@/$reader_group/" disko.service > /etc/systemd/system/disko.service
systemctl daemon-reload
systemctl enable disko.service
systemctl restart disko.service
echo 'Disko is running at http://0.0.0.0:7331. Use: disko status; disko du /; disko largest'
