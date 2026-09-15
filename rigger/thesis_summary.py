"""AI-written running summary for a thesis, grounded in accepted evidence.

The *state* is always computed deterministically by :mod:`rigger.thesis_health`;
this module only drafts the prose around it, citing accepted evidence ids. The
LLM never decides whether a thesis is "right".
"""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from pydantic import BaseModel, Field

from rigger.evidence import cite
from rigger.theses import Thesis, accepted_items, get_thesis
from rigger.thesis_health import HealthState, compute_health

SUMMARY_TEMPLATE = "thesis_summary_v1.j2"
PROMPT_VERSION = SUMMARY_TEMPLATE.removesuffix(".j2")


class SummaryDraft(BaseModel):
    summary: str
    strongest_support: str = ""
    strongest_counter: str = ""
    unknowns: list[str] = Field(default_factory=list)
    citations: list[str] = Field(default_factory=list)


class ThesisSummary(BaseModel):
    thesis_id: str
    claim: str
    state: HealthState
    summary: str
    strongest_support: str
    strongest_counter: str
    unknowns: list[str]
    citations: tuple[str, ...]
    as_of: datetime


def _draft_vars(
    thesis: Thesis,
    state: HealthState,
    tilt: float,
    counts: dict[str, int],
    items: list[dict[str, str]],
) -> dict[str, Any]:
    return {
        "claim": thesis.claim,
        "scope": thesis.scope,
        "assumptions": thesis.assumptions,
        "falsifiers": thesis.falsifiers,
        "time_horizon": thesis.time_horizon,
        "state": state,
        "tilt": tilt,
        "support": counts["support"],
        "against": counts["against"],
        "neutral": counts["neutral"],
        "items": items,
    }


async def summarize_thesis(rig: Any, thesis_id: str) -> ThesisSummary:
    """Run the summary model over a thesis's accepted evidence.

    When there is no accepted evidence, returns an ``emerging`` summary without
    calling the model. Otherwise the deterministic health is computed and passed
    into the prompt as fact; the model drafts prose that cites accepted ids, and
    unmatched citation ids are dropped.
    """
    from rigger.llm import structured as structured_mod
    from rigger.llm.router import model_for

    engine = rig.engine
    thesis = get_thesis(engine, thesis_id)
    linked = accepted_items(engine, thesis_id)

    if not linked:
        return ThesisSummary(
            thesis_id=thesis.id,
            claim=thesis.claim,
            state="emerging",
            summary="No accepted evidence yet — accept candidate evidence to populate this summary.",
            strongest_support="",
            strongest_counter="",
            unknowns=[],
            citations=(),
            as_of=datetime.now(UTC),
        )

    paired = [(item, row.side) for item, row in linked]
    health = compute_health(thesis, paired, now=datetime.now(UTC))
    counts = {"support": 0, "against": 0, "neutral": 0}
    for _, row in linked:
        counts[row.side] = counts[row.side] + 1

    items = [
        {
            "id": item.id,
            "side": row.side,
            "note": row.note,
            "cite": cite(item),
        }
        for item, row in linked
    ]

    draft, _result = await structured_mod.structured(
        rig.llm,
        task="thesis_summary",
        model=model_for(rig.cfg, "thesis_summary"),
        template=SUMMARY_TEMPLATE,
        vars=_draft_vars(thesis, health.state, health.tilt, counts, items),
        schema=SummaryDraft,
    )

    accepted_ids = {item.id for item, _ in linked}
    seen: set[str] = set()
    citations: list[str] = []
    for cid in draft.citations:
        if cid in accepted_ids and cid not in seen:
            seen.add(cid)
            citations.append(cid)

    return ThesisSummary(
        thesis_id=thesis.id,
        claim=thesis.claim,
        state=health.state,
        summary=draft.summary,
        strongest_support=draft.strongest_support,
        strongest_counter=draft.strongest_counter,
        unknowns=draft.unknowns,
        citations=tuple(citations),
        as_of=health.computed_at,
    )
