"""Core JSON helpers for list-valued columns."""

from __future__ import annotations

import json
from typing import Any


def to_json(value: list[str]) -> str:
    return json.dumps(value)


def from_json(value: str | None) -> list[str]:
    if not value:
        return []
    try:
        parsed: Any = json.loads(value)
    except json.JSONDecodeError:
        return []
    if isinstance(parsed, list):
        return [str(x) for x in parsed]
    return []
