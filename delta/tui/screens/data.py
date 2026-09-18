"""Compatibility entrypoint for the Research evidence view."""

from delta.tui.screens.research import Research


class Data(Research):
    name = "data"
    initial_view = "evidence"
