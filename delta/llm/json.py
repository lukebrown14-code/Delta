"""One JSON fence parser for every LLM output path.

Models sometimes wrap JSON in a markdown code fence (`` ```json ... ``` ``).
Both :mod:`delta.llm.structured` and :mod:`delta.chat` need the same fence
stripping before ``json.loads``; keeping two copies aloud them to drift. This
is the single implementation.
"""

from __future__ import annotations

import json
from typing import Any


def extract_json(text: str) -> Any:
    """``json.loads`` after stripping a markdown code fence; raises on failure.

    Returns the parsed value, or raises :class:`json.JSONDecodeError` when the
    text is not valid JSON after fence removal. Callers that must degrade
    gracefully catch the error themselves.
    """
    stripped = text.strip()
    if stripped.startswith("```"):
        body = stripped.split("```", 2)[1]
        if body.startswith("json"):
            body = body[4:]
        stripped = body.strip()
    return json.loads(stripped)