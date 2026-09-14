"""Stable content ids for rows without a natural key."""

from __future__ import annotations

import hashlib


def stable_id(*parts: object) -> str:
    """sha256 over the parts joined with NUL so ("ab","c") and ("a","bc") differ."""
    return hashlib.sha256("\0".join(str(p) for p in parts).encode()).hexdigest()
