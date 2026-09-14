"""Shared HTTP conventions for data plugins."""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version


def user_agent(extra: str = "") -> str:
    """``Rigger/<version> (<extra>)`` built from the installed package version."""
    try:
        ver = version("rigger")
    except PackageNotFoundError:  # pragma: no cover - running from a bare checkout
        ver = "0.0"
    return f"Rigger/{ver} ({extra})" if extra else f"Rigger/{ver}"
