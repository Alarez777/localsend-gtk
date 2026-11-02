# LocalSend GTK AppImage

`scripts/package-appimage.sh` creates a thin x86_64 or aarch64 AppImage. It
bundles the application binary, launcher, icon, desktop entry, AppStream
metadata, notices and licenses. It deliberately uses the host's GTK 4.12+ and
libadwaita 1.5+ libraries, matching the native packages and keeping the image
small. A fully self-contained GTK runtime is not included yet.

## Build

Install `appimagetool` or download `appimagetool-x86_64.AppImage`, then run:

```sh
APPIMAGETOOL=/path/to/appimagetool-x86_64.AppImage \
APPIMAGETOOL_EXTRACT=1 \
bash scripts/package-appimage.sh
```

`APPIMAGETOOL_EXTRACT=1` passes `--appimage-extract-and-run` to an AppImage
tool when FUSE is unavailable. It is not needed for a native `appimagetool`
binary. The result is written to:

```text
target/dist/localsend-gtk-v<version>-linux-<arch>.AppImage
```

and a matching `.sha256` file.

## Update information and zsync

The release workflow derives the AppImage update source from the repository
that triggered the build, so forks and upstream need no hardcoded owner:

```text
gh-releases-zsync|OWNER|REPO|latest|localsend-gtk-*-linux-<arch>.AppImage.zsync
```

It embeds that string with `appimagetool -u` and publishes the generated
`.AppImage.zsync` next to the AppImage. Gearlever and AppImageUpdate read the
embedded information, consult the latest published GitHub release and download
only the changed blocks.

For local release testing, provide the update information explicitly:

```sh
APPIMAGETOOL=/path/to/appimagetool-x86_64.AppImage \
APPIMAGETOOL_EXTRACT=1 \
APPIMAGE_UPDATE_INFO="gh-releases-zsync|Alarez777|localsend-gtk|latest|localsend-gtk-*-linux-x86_64.AppImage.zsync" \
bash scripts/package-appimage.sh
```

`zsyncmake` must be installed. Without `APPIMAGE_UPDATE_INFO` or
`GITHUB_REPOSITORY`, the script creates a plain AppImage and no `.zsync`, which
is useful for local testing.

The GitHub Actions release keeps the release in **draft** until it is validated
and published. Gearlever only sees the update after publication, because it
uses the latest published release.

## Run

```sh
chmod +x target/dist/localsend-gtk-*.AppImage
./target/dist/localsend-gtk-*.AppImage
```

Without FUSE, use `--appimage-extract-and-run`.

## Desktop integration

The AppImage runtime sets `APPIMAGE` to the outer AppImage path. LocalSend GTK
uses that path for autostart, file manager integrations and the optional desktop
entry, so they keep working after the temporary AppImage mount disappears. If
the AppImage file is moved, reinstall those integrations from Settings.

The AppImage contains `org.localsend.localsend_gtk.png`, a compatibility
`localsend-gtk.png` alias and `.DirIcon`. Its embedded desktop file includes
`StartupWMClass=org.localsend.localsend_gtk`; desktop integration tools such as
Gearlever copy this key and can then associate the running window with the
launcher, so the dock icon and **Pin to dock** work. After replacing an
AppImage that was already integrated, remove and re-add the integration in the
tool so it reads the new desktop file.

To make the application icon appear in the menu, dock and window, open
**Settings → Desktop integration → Application menu and icon**. That action
installs:

- `~/.local/share/applications/org.localsend.localsend_gtk.desktop`
- `~/.local/share/icons/hicolor/512x512/apps/org.localsend.localsend_gtk.png`

The launcher uses the outer `.AppImage` path and the localized file-manager
action name. Nautilus may still show a generic icon for the `.AppImage` file
itself unless AppImageLauncher, Gearlever or a compatible thumbnailer is
installed; that is independent of the application icon.

File manager integrations and native file dialogs still need the host file
manager, `xdg-desktop-portal` and its desktop backend. The AppImage cannot write
into the host's data directories from a Flatpak sandbox; it is a normal host
process, not a Flatpak. The optional Nautilus direct-menu extension needs the
host's `nautilus-python` bridge; the Scripts integration does not.
