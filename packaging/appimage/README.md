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

## Run

```sh
chmod +x target/dist/localsend-gtk-*.AppImage
./target/dist/localsend-gtk-*.AppImage
```

Without FUSE, use `--appimage-extract-and-run`.

## Desktop integration

The AppImage runtime sets `APPIMAGE` to the outer AppImage path. LocalSend GTK
uses that path for autostart and file manager integrations, so they keep working
after the temporary AppImage mount disappears. If the AppImage file is moved,
reinstall those integrations from Settings.

File manager integrations and native file dialogs still need the host file
manager, `xdg-desktop-portal` and its desktop backend. The AppImage cannot write
into the host's data directories from a Flatpak sandbox; it is a normal host
process, not a Flatpak.
