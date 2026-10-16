#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
install_service=false
install_deps=true
run_tests=false
for arg in "$@"; do
  case "$arg" in
    --install) install_service=true ;;
    --no-deps) install_deps=false ;;
    --test) run_tests=true ;;
    -h|--help)
      cat <<'HELP'
Usage: ./build.sh [--install] [--test] [--no-deps]

Installs missing Linux build prerequisites and Rust, then builds a locked release.
  --install   Also install/restart the systemd daemon for the current user.
  --test      Run the integration tests before building.
  --no-deps   Skip dependency installation (useful for CI/prepared machines).

Supports apt, pacman, dnf, zypper and apk. Package operations may invoke sudo.
Rust is installed via the official HTTPS rustup installer only if missing.
HELP
      exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done
[[ "$(uname -s)" == Linux ]] || { echo 'Disko currently supports Linux.' >&2; exit 1; }
as_root() {
  if (( EUID == 0 )); then "$@"; else sudo -- "$@"; fi
}
if $install_deps; then
  if ! command -v cc >/dev/null || ! command -v pkg-config >/dev/null || ! pkg-config --exists sqlite3 || ! command -v curl >/dev/null; then
    echo 'Installing missing compiler, SQLite development files, pkg-config and HTTPS tools…'
    if command -v apt-get >/dev/null; then
      as_root apt-get update
      as_root apt-get install -y build-essential pkg-config libsqlite3-dev curl ca-certificates
    elif command -v pacman >/dev/null; then
      as_root pacman -S --needed --noconfirm base-devel pkgconf sqlite curl ca-certificates
    elif command -v dnf >/dev/null; then
      as_root dnf install -y gcc gcc-c++ make pkgconf-pkg-config sqlite-devel curl ca-certificates
    elif command -v zypper >/dev/null; then
      as_root zypper --non-interactive install gcc gcc-c++ make pkg-config sqlite3-devel curl ca-certificates
    elif command -v apk >/dev/null; then
      as_root apk add build-base pkgconf sqlite-dev curl ca-certificates
    else
      echo 'Install a C compiler, pkg-config, SQLite development files and curl, then rerun.' >&2
      exit 1
    fi
  fi
  if ! command -v cargo >/dev/null || ! command -v rustc >/dev/null; then
    if [[ -f "${CARGO_HOME:-$HOME/.cargo}/env" ]]; then
      source "${CARGO_HOME:-$HOME/.cargo}/env"
    fi
    if ! command -v cargo >/dev/null || ! command -v rustc >/dev/null; then
      echo 'Installing Rust using https://sh.rustup.rs …'
      disko_rustup_script="$(mktemp)"
      trap 'rm -f -- "$disko_rustup_script"' EXIT
      curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs -o "$disko_rustup_script"
      sh "$disko_rustup_script" -y --profile minimal --default-toolchain stable
      source "${CARGO_HOME:-$HOME/.cargo}/env"
    fi
  fi
fi
for tool in cc pkg-config cargo rustc; do
  command -v "$tool" >/dev/null || { echo "Missing $tool; run ./build.sh with dependency setup enabled." >&2; exit 1; }
done
pkg-config --exists sqlite3 || { echo 'Missing SQLite development files.' >&2; exit 1; }
read -r disko_version < VERSION
disko_manifest_version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -n 1)"
[[ "$disko_version" == "$disko_manifest_version" ]] || { echo 'VERSION and Cargo.toml disagree.' >&2; exit 1; }
echo "Building Disko $disko_version with $(rustc --version)"
if $run_tests; then cargo test --locked; fi
cargo build --release --locked
echo "Built: $PWD/target/release/disko"
if $install_service; then
  command -v systemctl >/dev/null || { echo '--install requires systemd; run the binary manually on other init systems.' >&2; exit 1; }
  disko_reader="${SUDO_USER:-$(id -un)}"
  [[ "$disko_reader" != root ]] || { echo 'Run ./build.sh --install as the user who should read the index.' >&2; exit 1; }
  as_root bash ./install.sh "$disko_reader"
fi
