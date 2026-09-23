"""Bull / bear / neutral news stance via the Jev decision model.

One Jev choice question per news item per instrument: does the item read
bullish, bearish, or neutral for that stock? A judgment is keyed by
instrument plus evidence id, so a re-run classifies only new items, and every
stance stays traceable to the news row it came from. Jev returns
probabilities and no text, so the evidence itself is the explanation: reports
and screens that mention a stance cite the underlying evidence ids.
"""

from __future__ import annotations

import json
import logging
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Any

from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from delta.core.db import NewsItemTable, SentimentTable
from delta.core.ids import stable_id
from delta.core.json import from_json
from delta.core.time import parse_date, to_utc
from delta.llm.jev import PROMPT_VERSION, ChoiceQuestion, JevClient, JevError

logger = logging.getLogger(__name__)

SENTIMENT_TASK = "sentiment"
STANCES: tuple[str, ...] = ("bull", "bear", "neutral")
STANCE_VALUES: dict[str, float] = {"bull": 1.0, "bear": -1.0, "neutral": 0.0}

#: Lookback when ``since`` is not given; matches the extract stage's default.
DEFAULT_SINCE_DAYS = 14

#: Recency half-life for weighting: a week-old article counts half as much
#: as the same article published today, all else equal.
HALF_LIFE_DAYS = 7.0

STANCE_QUESTION = ChoiceQuestion(
    key="stance",
    instructions=(
        "Reading only this news item, does it read bullish or bearish for the stock?"
    ),
    criteria={
        "bull": (
            "Clearly positive for the stock: beats, upgrades, growth, "
            "contract wins, strong demand."
        ),
        "bear": (
            "Clearly negative for the stock: misses, guidance cuts, losses, "
            "lawsuits, weak demand."
        ),
        "neutral": "Mixed, routine, or without a clear effect on the stock.",
    },
)


@dataclass(frozen=True)
class SentimentSummary:
    """Recency- and confidence-weighted stance over a window."""

    instrument_id: str
    days: int
    score: float
    bull: int
    bear: int
    neutral: int


def stance_of(answer: Any) -> str:
    """The answer's choice as a stance; anything unexpected reads neutral."""
    choice = str(answer.get("choice", "neutral")) if isinstance(answer, dict) else "neutral"
    return choice if choice in STANCES else "neutral"


def judgment_id(instrument_id: str, evidence_id: str) -> str:
    return stable_id(instrument_id, evidence_id)


def _unclassified(engine: Engine, since: datetime) -> list[tuple[NewsItemTable, str]]:
    """News published since ``since`` paired with instruments not yet judged."""
    seen: set[tuple[str, str]] = set()
    with Session(engine) as session:
        for row in session.exec(select(SentimentTable)).all():
            seen.add((row.instrument_id, row.evidence_id))
        news = session.exec(
            select(NewsItemTable)
            .where(NewsItemTable.published >= since)
            .order_by(NewsItemTable.published.asc())  # type: ignore[attr-defined]
        ).all()
    pairs: list[tuple[NewsItemTable, str]] = []
    for item in news:
        for instrument_id in from_json(item.instrument_ids):
            if (instrument_id, item.id) not in seen:
                seen.add((instrument_id, item.id))
                pairs.append((item, instrument_id))
    return pairs


def _state(item: NewsItemTable, instrument_id: str, symbol: str) -> dict[str, Any]:
    return {
        "instrument_id": instrument_id,
        "symbol": symbol,
        "published": to_utc(item.published).isoformat(),
        "source": item.source,
        "title": item.title,
        "body": item.body or "",
        "evidence_id": item.id,
    }


def _row(
    item: NewsItemTable, instrument_id: str, model: str, answer: Any
) -> SentimentTable:
    probabilities = answer.get("probabilities") if isinstance(answer, dict) else None
    weights = (
        {str(k): float(v) for k, v in probabilities.items()} if isinstance(probabilities, dict) else {}
    )
    try:
        confidence = float(answer.get("confidence", 0.0))
    except (TypeError, ValueError):
        confidence = 0.0
    return SentimentTable(
        id=judgment_id(instrument_id, item.id),
        instrument_id=instrument_id,
        evidence_id=item.id,
        ts=to_utc(item.published),
        stance=stance_of(answer),
        confidence=max(0.0, min(confidence, 1.0)),
        probabilities=json.dumps(weights),
        model=model,
        prompt_version=PROMPT_VERSION,
    )


async def classify_news(
    delta: Any,
    *,
    since: str | None = None,
    log: Callable[[str], None] | None = None,
) -> list[SentimentTable]:
    """Ask Jev for a stance on every unclassified news item; persist judgments.

    The model comes from ``[llm.routing].sentiment``. A failed request skips
    its item; the next run picks it up, since only classified items are
    excluded.
    """
    from delta.llm.router import model_for

    say = log or (lambda _message: None)
    model = model_for(delta.cfg, SENTIMENT_TASK)
    since_dt = (
        parse_date(since) if since else datetime.now(UTC) - timedelta(days=DEFAULT_SINCE_DAYS)
    )
    pairs = _unclassified(delta.engine, since_dt)
    if not pairs:
        return []
    symbols = {inst.id: inst.symbol for inst in delta.universe()}
    client = JevClient(delta.settings.openrouter_api_key, delta.engine)
    stored: list[SentimentTable] = []
    for item, instrument_id in pairs:
        symbol = symbols.get(instrument_id, instrument_id.split(":", 1)[-1])
        try:
            decision = await client.decide(
                task=SENTIMENT_TASK,
                model=model,
                state=_state(item, instrument_id, symbol),
                questions=[STANCE_QUESTION],
            )
        except JevError:
            logger.warning("jev stance failed for %s/%s; skipping", instrument_id, item.id)
            continue
        answer = decision.answers.get("stance") or {}
        row = _row(item, instrument_id, decision.model, answer)
        with Session(delta.engine) as session:
            session.merge(row)
            session.commit()
        stored.append(row)
    say(
        f"[green]Classified {len(stored)} news stances via {model} "
        f"(bull/bear/neutral).[/green]"
    )
    return stored


def stock_sentiment(
    engine: Engine,
    instrument_id: str,
    *,
    days: int = 7,
    now: datetime | None = None,
) -> SentimentSummary | None:
    """Weighted stance summary over the last ``days`` days; None with no rows.

    Weight is confidence times a recency half-life, so a confident fresh
    article moves the score more than a timid stale one. The score runs
    -1 (all bear) to 1 (all bull); the counts are unweighted.
    """
    now = to_utc(now) if now is not None else datetime.now(UTC)
    floor = now - timedelta(days=days)
    with Session(engine) as session:
        rows = list(
            session.exec(
                select(SentimentTable)
                .where(SentimentTable.instrument_id == instrument_id)
                .where(SentimentTable.ts >= floor)
                .where(SentimentTable.ts <= now)
            ).all()
        )
    if not rows:
        return None
    weight_sum = 0.0
    score_sum = 0.0
    counts = dict.fromkeys(STANCES, 0)
    for row in rows:
        age_days = (now - to_utc(row.ts)).total_seconds() / 86400.0
        weight = 0.5 ** (age_days / HALF_LIFE_DAYS) * max(row.confidence, 0.0)
        weight_sum += weight
        score_sum += STANCE_VALUES.get(row.stance, 0.0) * weight
        if row.stance in counts:
            counts[row.stance] += 1
    score = score_sum / weight_sum if weight_sum > 0 else 0.0
    return SentimentSummary(
        instrument_id=instrument_id,
        days=days,
        score=score,
        bull=counts["bull"],
        bear=counts["bear"],
        neutral=counts["neutral"],
    )
