#!/bin/sh
# Install the yan toolchain from a GitHub release.
#   curl -fsSL https://github.com/zaofang/yan/releases/latest/download/install.sh | sh
# Environment: YAN_VERSION (tag, default latest), YAN_HOME (default ~/.yan),
# YAN_BASE_URL (release download root, for mirrors and tests).
set -eu

repo=zaofang/yan
home=${YAN_HOME:-$HOME/.yan}
if [ -n "${YAN_BASE_URL:-}" ]; then
    base=$YAN_BASE_URL
elif [ -n "${YAN_VERSION:-}" ]; then
    base=https://github.com/$repo/releases/download/$YAN_VERSION
else
    base=https://github.com/$repo/releases/latest/download
fi

fail() { echo "yan install: $*" >&2; exit 1; }

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu ;;
    *) fail "no release for $(uname -s) $(uname -m)" ;;
esac

if command -v sha256sum >/dev/null; then
    sha() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null; then
    sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    fail "need sha256sum or shasum"
fi

fetch() {
    case "$1" in
        http://* | https://*) curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1" ;;
        *) cp "$1" "$2" ;;
    esac
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
archive=yan-$target.tar.gz
fetch "$base/$archive" "$tmp/$archive"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS"

want=$(awk -v f="$archive" '$2 == f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$want" ] || fail "$archive is not listed in SHA256SUMS"
[ "$(sha "$tmp/$archive")" = "$want" ] || fail "$archive does not match SHA256SUMS"

if command -v cosign >/dev/null; then
    fetch "$base/SHA256SUMS.sigstore.json" "$tmp/SHA256SUMS.sigstore.json"
    cosign verify-blob "$tmp/SHA256SUMS" \
        --bundle "$tmp/SHA256SUMS.sigstore.json" \
        --certificate-identity-regexp "^https://github.com/$repo/.github/workflows/release.yml@refs/tags/" \
        --certificate-oidc-issuer https://token.actions.githubusercontent.com >/dev/null 2>&1 ||
        fail "SHA256SUMS signature does not verify"
    echo "verified the Sigstore signature of SHA256SUMS"
fi

mkdir -p "$home/bin"
tar -xzf "$tmp/$archive" -C "$home/bin"
echo "installed $("$home/bin/yan" --version 2>/dev/null || echo yan) to $home/bin"
case ":$PATH:" in
    *":$home/bin:"*) ;;
    *) echo "add $home/bin to PATH: export PATH=\"$home/bin:\$PATH\"" ;;
esac
