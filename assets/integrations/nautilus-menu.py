#!/usr/bin/env python3
# X-LocalSend-GTK-Owner: org.localsend.localsend_gtk
import subprocess

from gi.repository import GObject, Nautilus

BINARY = @LOCALSEND_GTK_BINARY@
ACTION_LABEL = @LOCALSEND_GTK_ACTION@
NOT_FOUND_MESSAGE = @LOCALSEND_GTK_NOT_FOUND@


def notify(message):
    try:
        subprocess.Popen(["notify-send", "LocalSend", message, "-i", "localsend-gtk"])
    except OSError:
        pass


class LocalSendMenuProvider(GObject.GObject, Nautilus.MenuProvider):
    def get_file_items(self, files):
        paths = []
        for file in files:
            if file.get_uri_scheme() != "file":
                return []
            location = file.get_location()
            path = location.get_path() if location is not None else None
            if not path:
                return []
            paths.append(path)
        if not paths:
            return []
        item = Nautilus.MenuItem(
            name="LocalSendGTK::SendWithLocalSend",
            label=ACTION_LABEL,
            tip=ACTION_LABEL,
            icon="",
        )
        item.connect("activate", self.activate, paths)
        return [item]

    # Nautilus warns if this method is missing.
    def get_background_items(self, current_folder):
        return []

    def activate(self, menu, paths):
        try:
            subprocess.Popen([BINARY, "--", *paths])
        except OSError:
            notify(NOT_FOUND_MESSAGE)
