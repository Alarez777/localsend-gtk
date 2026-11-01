#!/usr/bin/env bash
set -euo pipefail

# Resolve relative prefixes against the caller's directory, not the archive.
PREFIX="$(realpath -m -- "${1:-/usr/local}")"
case "$PREFIX" in
    *'='*|*$'\n'*|*$'\r'*)
        echo "The installation prefix cannot contain '=', a newline or a carriage return." >&2
        exit 1
        ;;
esac
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

desktop_string() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//$'\t'/\\t}"
    printf '%s' "$value"
}

desktop_exec() {
    local value="$1"
    # Like the autostart entry, defer paths containing '%' until field codes are
    # expanded: GIO otherwise checks for a literal '%%' executable first.
    if [[ "$value" == *%* ]]; then
        printf '/usr/bin/env -- '
    fi
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    value="${value//\`/\\\`}"
    value="${value//\$/\\\$}"
    value="${value//%/%%}"
    printf '"%s"' "$(desktop_string "$value")"
}

install_desktop() {
    local source="$1" destination="$2" mode="$3" line
    install -D -m "$mode" "$source" "$destination"
    # A custom prefix need not be on the desktop session's PATH or icon path.
    while IFS= read -r line || [[ -n "$line" ]]; do
        case "$line" in
            Exec=*) printf 'Exec=%s %%U\n' "$(desktop_exec "$PREFIX/bin/localsend-gtk")" ;;
            Icon=*) printf 'Icon=%s\n' "$(desktop_string "$PREFIX/share/icons/hicolor/512x512/apps/localsend-gtk.png")" ;;
            *) printf '%s\n' "$line" ;;
        esac
    done < "$source" > "$destination"
}

echo "Installing LocalSend GTK to $PREFIX..."
install -D -m 755 bin/localsend-gtk "$PREFIX/bin/localsend-gtk"
install_desktop share/applications/org.localsend.localsend_gtk.desktop \
    "$PREFIX/share/applications/org.localsend.localsend_gtk.desktop" 644
install -D -m 644 share/icons/hicolor/512x512/apps/localsend-gtk.png "$PREFIX/share/icons/hicolor/512x512/apps/localsend-gtk.png"
mkdir -p "$PREFIX/share/doc"
cp -R share/doc/localsend-gtk "$PREFIX/share/doc/"
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$PREFIX/share/applications"
fi
echo "Installed. GTK4 >= 4.12, libadwaita >= 1.5 and compatible system libraries are required."
echo "Your desktop must include $PREFIX/share in its XDG data search paths to find the launcher."
echo "File manager integrations are optional and are installed from Settings."
