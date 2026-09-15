"""Tiny on-disk session state: when the user last opened Rigger.

Kept next to the SQLite file rather than in ``config.toml`` — the app writes
this, the user does not edit it.
"""

from __future__ import annotations

import json
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from rigger.core.time import to_utc

STATE_NAME = ".rigger_state.json"

#: How far back a first run looks, so it shows something without replaying history.
FIRST_RUN_WINDOW = timedelta(days=7)


def state_path(cfg: Any) -> Path:
    return Path(getattr(cfg, "db_path", "data/rigger.db")).parent / STATE_NAME


def read_last_seen(cfg: Any) -> datetime:
    """When the user last opened Rigger, or a week ago if that is unknown."""
    try:
        raw = json.loads(state_path(cfg).read_text(encoding="utf-8"))
        return to_utc(datetime.fromisoformat(raw["last_seen"]))
    except (OSError, ValueError, KeyError, TypeError):
        return datetime.now(UTC) - FIRST_RUN_WINDOW


def write_last_seen(cfg: Any, when: datetime | None = None) -> None:
    """Record this visit. Never raises — a read-only disk must not stop the app."""
    path = state_path(cfg)
    payload = json.dumps({"last_seen": (when or datetime.now(UTC)).isoformat()})
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(payload, encoding="utf-8")
    except OSError:
        pass
