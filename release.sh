#!/usr/bin/env bash
# Build the static Linux binary and publish it as a GitHub release (no CI involved).
#
#   ./release.sh          build, package and upload release v$(cat VERSION)
#   ./release.sh build    only build and package into dist/
#
# The binary is linked statically against musl with SQLite compiled in, so it runs
# on any x86_64 Linux. The asset name has no version in it, so this link always
# serves the newest release:
#   https://github.com/olibuijr/disko/releases/latest/download/disko-x86_64-linux.tar.gz
# Needs: rustup target x86_64-unknown-linux-musl, musl-gcc, gh (logged in).
set -euo pipefail
cd -- "$(dirname -- "$0")"
version=$(cat VERSION)
tag=v$version
target=x86_64-unknown-linux-musl
name=disko-x86_64-linux

die() { echo "release: $*" >&2; exit 1; }

build() {
    grep -q "^version = \"$version\"" Cargo.toml || die "Cargo.toml version differs from VERSION"
    cargo build --release --locked --target $target --features bundled
    bin=target/$target/release/disko
    [[ "$("$bin" --version)" == "disko $version" ]] || die "binary does not report $version"
    rm -rf "dist/$name" "dist/$name.tar.gz"
    mkdir -p "dist/$name"
    cp "$bin" install.sh disko.service LICENSE README.md "dist/$name/"
    tar -C dist --owner=0 --group=0 -czf "dist/$name.tar.gz" "$name"
    (cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
    echo "packaged dist/$name.tar.gz ($(du -h "dist/$name.tar.gz" | cut -f1))"
}

notes() {
    # This version's section of the changelog, without its heading.
    awk -v v="$version" '$0 ~ "^## \\[" v "\\]" {on=1; next} /^## \[/ {on=0} on' CHANGELOG.md
    cat <<EOF

## Install

\`\`\`sh
curl -LO https://github.com/olibuijr/disko/releases/latest/download/$name.tar.gz
tar xzf $name.tar.gz && cd $name
sudo ./install.sh "\$USER"
\`\`\`

A static x86_64 Linux binary (musl, SQLite included). Check the download with $name.tar.gz.sha256.
EOF
}

publish() {
    [[ -z "$(git status --porcelain -- src Cargo.toml Cargo.lock VERSION)" ]] || die "commit the source first"
    git push -q origin HEAD
    git rev-parse -q --verify "refs/tags/$tag" >/dev/null || git tag -a "$tag" -m "Disko $version"
    git push -q origin "$tag"
    if gh release view "$tag" >/dev/null 2>&1; then
        gh release upload "$tag" "dist/$name.tar.gz" "dist/$name.tar.gz.sha256" --clobber
    else
        gh release create "$tag" "dist/$name.tar.gz" "dist/$name.tar.gz.sha256" --title "Disko $version" --notes "$(notes)" --latest
    fi
    echo "released $tag: https://github.com/olibuijr/disko/releases/tag/$tag"
}

case ${1:-release} in
    build) build ;;
    release) build; publish ;;
    *) die "unknown command $1; see the top of release.sh" ;;
esac
