#!/usr/bin/env bash
# Build the macOS .dmg locally and (optionally) publish it to a GitHub
# Release, so the fork always has the latest downloadable build WITHOUT
# committing the binary into git history.
#
# Why a Release and not a committed file: a .dmg is a ~tens-of-MB binary.
# Git keeps every version forever and binaries don't delta-compress, so
# committing each build would bloat the repo permanently and fight the
# clean upstream rebases this fork relies on. GitHub Releases stores the
# artifact alongside the repo, downloadable, but out of git history.
#
# Output: dist/oryxis-macos-aarch64.dmg (dist/ is git-ignored).
#
# Usage:
#   scripts/build-dmg.sh            # build the .dmg into dist/
#   scripts/build-dmg.sh --release  # build, then upload to a GitHub Release
#
# The --release step needs the `gh` CLI authenticated with push access to
# the `origin` remote. It tags a rolling `latest-local` release and
# replaces its asset each run, so the "latest build" link is stable.

set -euo pipefail
cd "$(dirname "$0")/.."

APP="Oryxis.app"
DMG="dist/oryxis-macos-aarch64.dmg"
VOL="Oryxis"

mkdir -p dist

echo "==> Building release binary"
cargo build --locked --release

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/oryxis "$APP/Contents/MacOS/oryxis"
cp resources/Oryxis.icns "$APP/Contents/Resources/Oryxis.icns"
# Version from the app crate's Cargo.toml (first `version = "..."`).
VERSION="$(grep -m1 '^version' crates/oryxis-app/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
VERSION="${VERSION:-0.0.0}"
sed "s/__VERSION__/${VERSION}/g" resources/Info.plist > "$APP/Contents/Info.plist"

echo "==> Ad-hoc signing (enough to run locally; not Developer-ID/notarized)"
codesign --force --deep --sign - "$APP"

echo "==> Building $DMG"
STAGING="$(mktemp -d)/dmg"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
cp resources/INSTALL-macOS.md "$STAGING/"
# Size the image from measured content + 300 MB headroom (hdiutil's own
# estimate runs the volume too tight and fails with a misleading
# "No space left on device"). UDZO compresses the slack away.
SIZE_MB=$(( $(du -sm "$STAGING" | cut -f1) + 300 ))
rm -f "$DMG"
hdiutil create -volname "$VOL" -srcfolder "$STAGING" \
  -size "${SIZE_MB}m" -ov -format UDZO "$DMG"
rm -rf "$STAGING" "$APP"

echo "==> Built $DMG (v${VERSION})"

if [[ "${1:-}" == "--release" ]]; then
  if ! command -v gh >/dev/null 2>&1; then
    echo "error: --release needs the GitHub CLI (gh). Install it or drop --release." >&2
    exit 1
  fi
  TAG="latest-local"
  TITLE="Latest local build (v${VERSION})"
  echo "==> Publishing $DMG to GitHub Release '$TAG'"
  # Recreate the rolling release so the asset is always the newest build
  # and the download URL stays stable. Best-effort delete first.
  gh release delete "$TAG" --yes --cleanup-tag 2>/dev/null || true
  gh release create "$TAG" "$DMG" \
    --title "$TITLE" \
    --notes "Rolling local build of Oryxis for Apple Silicon. Ad-hoc signed (not notarized); on first launch, right-click the app and choose Open. Built $(date -u '+%Y-%m-%d %H:%M UTC')."
  echo "==> Published. Latest build: gh release view $TAG"
fi
