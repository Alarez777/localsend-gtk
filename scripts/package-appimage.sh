#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

for tool in cargo python3 install strip desktop-file-validate mksquashfs; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "Required AppImage packaging tool missing: $tool" >&2
        exit 1
    }
done

APPIMAGETOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [[ -z "$APPIMAGETOOL" ]]; then
    echo "Set APPIMAGETOOL to appimagetool or appimagetool-x86_64.AppImage." >&2
    exit 1
fi
if [[ ! -x "$APPIMAGETOOL" ]]; then
    echo "APPIMAGETOOL is not executable: $APPIMAGETOOL" >&2
    exit 1
fi

VERSION="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "Expected a stable Cargo package version, got: $VERSION" >&2
    exit 1
}
ARCH="${ARCH:-$(uname -m)}"
case "$ARCH" in
    x86_64|aarch64) ;;
    *) echo "Unsupported AppImage architecture: $ARCH" >&2; exit 1 ;;
esac

cargo build --release --locked
BIN="$ROOT/target/release/localsend-gtk"
[[ -x "$BIN" ]] || { echo "Release binary is missing: $BIN" >&2; exit 1; }
strip -s "$BIN"

APPIMAGE_ROOT="$ROOT/target/appimage"
APPDIR="$APPIMAGE_ROOT/AppDir"
rm -rf "$APPIMAGE_ROOT"
mkdir -p "$APPDIR/usr/bin" \
    "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
    "$APPDIR/usr/share/metainfo" \
    "$APPDIR/usr/share/doc/localsend-gtk/localsend-rs" \
    "$APPDIR/usr/share/licenses/localsend-gtk"

install -m 755 "$BIN" "$APPDIR/usr/bin/localsend-gtk"
install -m 755 packaging/appimage/AppRun "$APPDIR/AppRun"
# AppImage tools and desktop environments resolve the embedded icon by the
# desktop file's Icon key. Keep the native alias too for the file manager.
sed 's/^Icon=.*/Icon=org.localsend.localsend_gtk/' \
    packaging/rpm/localsend-gtk.desktop > "$APPDIR/org.localsend.localsend_gtk.desktop"
install -m 644 "$APPDIR/org.localsend.localsend_gtk.desktop" \
    "$APPDIR/usr/share/applications/org.localsend.localsend_gtk.desktop"
install -m 644 assets/logo.png "$APPDIR/org.localsend.localsend_gtk.png"
install -m 644 assets/logo.png "$APPDIR/localsend-gtk.png"
ln -sfn org.localsend.localsend_gtk.png "$APPDIR/.DirIcon"
install -m 644 assets/logo.png \
    "$APPDIR/usr/share/icons/hicolor/512x512/apps/org.localsend.localsend_gtk.png"
install -m 644 assets/logo.png \
    "$APPDIR/usr/share/icons/hicolor/512x512/apps/localsend-gtk.png"
install -m 644 packaging/flatpak/org.localsend.localsend_gtk.metainfo.xml \
    "$APPDIR/usr/share/metainfo/org.localsend.localsend_gtk.appdata.xml"
install -m 644 README.md LICENSE THIRD_PARTY_NOTICES.md \
    "$APPDIR/usr/share/doc/localsend-gtk/"
install -m 644 LICENSES/*.txt "$APPDIR/usr/share/licenses/localsend-gtk/"
install -m 644 vendor/localsend-rs/UPSTREAM.md \
    vendor/localsend-rs/THIRD_PARTY_NOTICES.md \
    "$APPDIR/usr/share/doc/localsend-gtk/localsend-rs/"

desktop-file-validate "$APPDIR/org.localsend.localsend_gtk.desktop"
mkdir -p "$ROOT/target/dist"
OUTPUT="$ROOT/target/dist/localsend-gtk-v${VERSION}-linux-${ARCH}.AppImage"
rm -f "$OUTPUT"

TOOL_ARGS=()
if [[ "${APPIMAGETOOL_EXTRACT:-0}" == "1" ]]; then
    TOOL_ARGS+=(--appimage-extract-and-run)
fi

VERSION="$VERSION" ARCH="$ARCH" \
    "$APPIMAGETOOL" "${TOOL_ARGS[@]}" "$APPDIR" "$OUTPUT"
chmod 755 "$OUTPUT"
CHECKSUM="$(sha256sum "$OUTPUT" | cut -d' ' -f1)"
printf '%s  %s\n' "$CHECKSUM" "$(basename "$OUTPUT")" > "$OUTPUT.sha256"
echo "Created $OUTPUT"
echo "The AppImage uses the system GTK4/libadwaita runtime; see packaging/appimage/README.md."
