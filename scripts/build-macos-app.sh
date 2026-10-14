#!/usr/bin/env bash
# Builds "SQL Job Monitor.app" (and optionally a .dmg) into dist/. Run on macOS.
#
#   bash scripts/build-macos-app.sh              # app for this Mac's architecture
#   bash scripts/build-macos-app.sh --universal  # Apple silicon + Intel in one binary
#   bash scripts/build-macos-app.sh --dmg        # also write a disk image
#   bash scripts/build-macos-app.sh --legacy-tls # OpenSSL 3, TLS 1.0 allowed (unpatched SQL Server 2012)
#
# Set CODESIGN_IDENTITY="Developer ID Application: ..." to sign for distribution;
# otherwise the bundle is ad-hoc signed, which is enough to run it on this Mac.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "error: build the .app on macOS (needs lipo, sips, iconutil, codesign)." >&2
    exit 1
fi

cd "$(dirname "$0")/.."
UNIVERSAL=0
DMG=0
LEGACY=0
for arg in "$@"; do
    case "$arg" in
        --universal) UNIVERSAL=1 ;;
        --dmg) DMG=1 ;;
        --legacy-tls) LEGACY=1 ;;
        *) echo "error: unknown option $arg" >&2; exit 2 ;;
    esac
done

APP_NAME="SQL Job Monitor"
BIN="sql-job-monitor"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
DIST="dist"
APP="$DIST/$APP_NAME.app"

if [[ "$LEGACY" == 1 ]]; then
    # Replaces rustls. Tiberius compiles only one TLS backend at a time.
    # openssl-src compiles OpenSSL 3 from source, which needs perl and make.
    for tool in perl make; do
        command -v "$tool" >/dev/null || {
            echo "error: --legacy-tls needs $tool (xcode-select --install)" >&2
            exit 1
        }
    done
    CARGO_TLS=(--no-default-features --features legacy-tls)
    echo "TLS backend: statically linked OpenSSL 3 at security level 0 (TLS 1.0 allowed; see README)"
    echo "warning: use this build only until the server has TLS 1.2." >&2
else
    CARGO_TLS=()
    echo "TLS backend: rustls (TLS 1.2 and 1.3)"
fi

if [[ "$UNIVERSAL" == 1 ]]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release "${CARGO_TLS[@]}" --target aarch64-apple-darwin
    cargo build --release "${CARGO_TLS[@]}" --target x86_64-apple-darwin
    mkdir -p target/universal
    lipo -create -output "target/universal/$BIN" \
        "target/aarch64-apple-darwin/release/$BIN" \
        "target/x86_64-apple-darwin/release/$BIN"
    BINARY="target/universal/$BIN"
else
    cargo build --release "${CARGO_TLS[@]}"
    BINARY="target/release/$BIN"
fi

if [[ "$LEGACY" == 1 ]]; then
    OPENSSL_VERSION="$(strings -a "$BINARY" | grep -m1 -E '^OpenSSL 3\.' || true)"
    if [[ -z "$OPENSSL_VERSION" ]]; then
        echo "error: $BINARY does not contain OpenSSL 3; the legacy TLS build is not active." >&2
        exit 1
    fi
    echo "Linked: $OPENSSL_VERSION"
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/$BIN"
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/app.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" assets/app.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

if [[ -n "${CODESIGN_IDENTITY:-}" ]]; then
    codesign --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$APP"
else
    codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

echo "Built $APP ($VERSION)"

if [[ "$DMG" == 1 ]]; then
    DMG_PATH="$DIST/sql-job-monitor-$VERSION-macos.dmg"
    rm -f "$DMG_PATH"
    STAGING="$(mktemp -d)"
    cp -R "$APP" "$STAGING/"
    ln -s /Applications "$STAGING/Applications"
    hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DMG_PATH" >/dev/null
    echo "Built $DMG_PATH"
fi
