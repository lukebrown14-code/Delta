"""Datetime helpers. SQLite hands back naive datetimes; treat them as UTC."""

from __future__ import annotations

from datetime import UTC, datetime


def to_utc(ts: datetime) -> datetime:
    """Return ``ts`` as an aware UTC datetime. Naive input is assumed to be UTC."""
    if ts.tzinfo is None:
        return ts.replace(tzinfo=UTC)
    return ts.astimezone(UTC)


def parse_date(text: str) -> datetime:
    """``YYYY-MM-DD`` -> aware UTC midnight."""
    return datetime.strptime(text, "%Y-%m-%d").replace(tzinfo=UTC)
