"""Sourced research reports: the AI synthesises gathered evidence, never invents it.

``build_report`` gathers a target's evidence, asks the ``report`` route for a
structured draft, then enforces the citation contract: any claim naming an
evidence id that was not gathered is dropped, and a draft left with no
substantive claims is rejected. Reports synthesise and challenge; they never
recommend buying or selling.
"""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field
from sqlalchemy.engine import Engine

from rigger.evidence import EvidenceItem, cite, evidence
from rigger.llm.router import model_for
from rigger.targets import WatchTarget

REPORT_TEMPLATE = "report_v1.j2"
PROMPT_VERSION = REPORT_TEMPLATE.removesuffix(".j2")

#: Claim-list draft fields paired with their markdown headings. A report with
#: none of these surviving validation has no substance and is rejected.
_SECTIONS: tuple[tuple[str, str], ...] = (
    ("bull", "Bull case"),
    ("bear", "Bear case"),
    ("risks", "Risks"),
    ("catalysts", "Catalysts"),
    ("sentiment_reasons", "Sentiment reasons"),
)


class Claim(BaseModel):
    """The smallest cited unit: one statement plus the evidence supporting it."""

    text: str
    evidence_ids: list[str] = Field(default_factory=list)


class ReportDraft(BaseModel):
    """The structured-call output shape for ``report_v1``."""

    summary: str
    bull: list[Claim] = Field(default_factory=list)
    bear: list[Claim] = Field(default_factory=list)
    risks: list[Claim] = Field(default_factory=list)
    catalysts: list[Claim] = Field(default_factory=list)
    unknowns: list[str] = Field(default_factory=list)
    sentiment: float = Field(ge=-1, le=1)
    sentiment_reasons: list[Claim] = Field(default_factory=list)


class Report(ReportDraft):
    """A validated draft bound to a target, a generation time, and its citations.

    ``citations`` maps each gathered evidence id to its cite text so rendering
    and the persisted file stay fully sourced even after the pool moves on.
    """

    target_id: str
    as_of: datetime
    prompt_version: str
    citations: dict[str, str] = Field(default_factory=dict)


def gather(
    target_id: str,
    engine: Engine,
    *,
    since: str | None = None,
    limit: int = 200,
) -> list[EvidenceItem]:
    """Evidence for one target id, newest first (thin wrapper over evidence())."""
    return evidence(engine, target=target_id, since=since, limit=limit)


def _supported(claims: list[Claim], gathered: set[str]) -> list[Claim]:
    """Keep claims citing at least one gathered id and no id outside the pool."""
    return [claim for claim in claims if claim.evidence_ids and set(claim.evidence_ids) <= gathered]


async def build_report(rig: Any, target_id: str, *, since: str | None = None) -> Report:
    """Synthesise a cited report for ``target_id`` from its gathered evidence.

    ``target_id`` is any id the evidence read model matches (an instrument id
    such as ``US:AAPL``). Raises ValueError when no evidence is gathered or when
    dropping unsupported claims leaves no substantive claim.
    """
    from rigger import services
    from rigger.llm import structured as structured_mod

    items = gather(target_id, rig.engine, since=since)
    if not items:
        raise ValueError(f"no evidence gathered for target {target_id!r}; gather evidence first")

    target = services.target_specs().get(target_id) or WatchTarget(
        id=target_id, kind="company", name=target_id
    )
    draft, _result = await structured_mod.structured(
        rig.llm,
        task="report",
        model=model_for(rig.cfg, "report"),
        template=REPORT_TEMPLATE,
        vars={
            "target": {"id": target.id, "name": target.name, "kind": target.kind},
            "items": [
                {
                    "id": item.id,
                    "cite": cite(item),
                    "kind": item.kind,
                    "ts": item.ts.isoformat(),
                    "body": item.body or "",
                }
                for item in items
            ],
        },
        schema=ReportDraft,
    )

    citations = {item.id: cite(item) for item in items}
    gathered = set(citations)
    kept = {field: _supported(getattr(draft, field), gathered) for field, _title in _SECTIONS}
    if not any(kept.values()):
        raise ValueError(
            f"report for target {target_id!r} has no claims supported by the gathered evidence"
        )
    return Report(
        target_id=target_id,
        as_of=datetime.now(UTC),
        prompt_version=PROMPT_VERSION,
        citations=citations,
        summary=draft.summary,
        bull=kept["bull"],
        bear=kept["bear"],
        risks=kept["risks"],
        catalysts=kept["catalysts"],
        unknowns=draft.unknowns,
        sentiment=draft.sentiment,
        sentiment_reasons=kept["sentiment_reasons"],
    )


def render_markdown(report: Report, *, interactive: bool = False) -> str:
    """Markdown with every claim followed by the cite lines of its evidence."""
    lines = [
        f"# Report — {report.target_id}",
        "",
        f"*as of {report.as_of:%Y-%m-%d %H:%M} UTC · prompt {report.prompt_version} · "
        f"sentiment {report.sentiment:.2f}*",
        "",
        "## Summary",
        "",
        report.summary,
        "",
    ]
    for field, title in _SECTIONS:
        claims: list[Claim] = getattr(report, field)
        lines += [f"## {title}", ""]
        if claims:
            for claim in claims:
                lines.append(f"- {claim.text}")
                lines.extend(
                    (
                        f"  - [Inspect source](evidence:{evidence_id}) — "
                        f"{report.citations.get(evidence_id, evidence_id)}"
                        if interactive
                        else f"  - {report.citations.get(evidence_id, evidence_id)}"
                    )
                    for evidence_id in claim.evidence_ids
                )
        else:
            lines.append("- none supported by the gathered evidence")
        lines.append("")
    lines += ["## Unknowns", ""]
    lines.extend(f"- {unknown}" for unknown in report.unknowns or ["none"])
    return "\n".join(lines) + "\n"


def write_report(report: Report, base_dir: str | Path) -> Path:
    """Write ``base_dir/<target_id>/<YYYY-MM-DD>.md`` and return its path."""
    target_dir = Path(base_dir) / report.target_id
    target_dir.mkdir(parents=True, exist_ok=True)
    path = target_dir / f"{report.as_of:%Y-%m-%d}.md"
    path.write_text(render_markdown(report), encoding="utf-8")
    path.with_suffix(".json").write_text(report.model_dump_json(indent=2), encoding="utf-8")
    return path
