"""Compatibility entrypoint for the Research report view."""

from delta.tui.screens.research import Research


class Reports(Research):
    name = "reports"
    initial_tab = "report"
