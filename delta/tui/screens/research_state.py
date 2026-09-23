"""Shared state and constants for the Research desk (split out of ``research.py``).

The screen file owns the desk's behaviour; this module holds the data the panes
render — evidence-kind colour tokens, pane widths, and the per-company view
state — so importing ``ResearchState`` does not drag in the whole screen.
"""

from __future__ import annotations

from dataclasses import dataclass, field

#: Evidence kind -> the theme token that colours its Type cell. The colour
#: sits on that cell alone, never the whole row, so the block cursor stays
#: readable over it.
KIND_TOKENS = {
    "news": "kind-news",
    "filing": "kind-filing",
    "event": "kind-event",
    "fundamental": "kind-fundamental",
    "bar": "kind-price",
}

#: What a kind is called on screen. The store calls a daily close a ``bar``;
#: nobody reading a research pane does.
KIND_LABELS = {"bar": "price", "fundamental": "fundam."}

#: Keys of ``EvidenceItem.raw`` that are plumbing, not evidence: they go to a
#: dim provenance footer instead of the value table.
INTERNAL_RAW = ("id", "content_hash", "extracted_by", "prompt_version", "evidence_ids")

#: Evidence kinds in the order ``k`` cycles them; label, filter value.
KINDS: tuple[tuple[str, str], ...] = (
    ("all", "all"),
    ("news", "news"),
    ("filings", "filing"),
    ("prices", "bar"),
    ("fundamentals", "fundamental"),
    ("events", "event"),
)

#: Pane widths at the wide layout, mirroring Theses: the picker and the
#: evidence list are fixed-shape columns, the report takes the rest.
COMPANY_WIDTH = 36
EVIDENCE_WIDTH = 40


@dataclass
class CompanyView:
    search: str = ""
    kind: str = "all"
    #: Price runs the reader has unfolded, by the key of their group row.
    expanded: set[str] = field(default_factory=set)
    limit: int = 200
    selected: str = ""
    inspected: str = ""
    report_y: float = 0
    list_y: float = 0
    preview_y: float = 0


@dataclass
class ResearchState:
    target: str = ""
    company: str = ""
    companies: dict[str, CompanyView] = field(default_factory=dict)
    busy: bool = False
    activity: str = ""
