"""Shared HTTP conventions for data plugins."""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version


def user_agent(extra: str = "") -> str:
    """``Delta/<version> (<extra>)`` built from the installed package version."""
    try:
        ver = version("delta")
    except PackageNotFoundError:  # pragma: no cover - running from a bare checkout
        ver = "0.0"
    return f"Delta/{ver} ({extra})" if extra else f"Delta/{ver}"
