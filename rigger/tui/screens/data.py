"""Compatibility entrypoint for the Research evidence view."""

from rigger.tui.screens.research import Research


class Data(Research):
    name = "data"
    initial_tab = "evidence"
