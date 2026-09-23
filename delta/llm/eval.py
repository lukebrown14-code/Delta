"""Offline evaluation harness: citation validity over golden evidence sets.

The project spec calls evaluation a prerequisite for ever revisiting trading:
nothing yet measures whether the research is any good. This harness is the
first, minimal slice of that — it measures the one property the citation
contract already promises, *citation validity*, against a golden evidence set.

It is deliberately pure and offline. A golden case names the evidence ids a
run is allowed to cite, and a result records which citations the model produced
and how many of them were valid. No provider is ever called; callers feed the
harness citations they already gathered from ``FakeLLM``-backed flows.
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass, field


def valid_citations(citations: Iterable[str], valid_ids: set[str]) -> list[str]:
    """The citations present in ``valid_ids``, de-duplicated, order preserved."""
    seen: set[str] = set()
    kept: list[str] = []
    for citation in citations:
        if citation in valid_ids and citation not in seen:
            seen.add(citation)
            kept.append(citation)
    return kept


def hallucinated_citations(citations: Iterable[str], valid_ids: set[str]) -> list[str]:
    """The citations not backed by ``valid_ids``, de-duplicated, order preserved."""
    seen: set[str] = set()
    dropped: list[str] = []
    for citation in citations:
        if citation not in valid_ids and citation not in seen:
            seen.add(citation)
            dropped.append(citation)
    return dropped


def citation_validity_rate(citations: Iterable[str], valid_ids: set[str]) -> float:
    """Fraction of produced citations backed by ``valid_ids``; 1.0 when none.

    A run with no citations has nothing invalid to flag, so it rates a perfect
    1.0 rather than dividing by zero.
    """
    produced = list(citations)
    if not produced:
        return 1.0
    return len(valid_citations(produced, valid_ids)) / len(produced)


@dataclass
class GoldenCase:
    """One golden scenario: the name, the evidence pool, and assertions."""

    name: str
    evidence_ids: set[str] = field(default_factory=set)
    #: Minimum citation-validity rate the case must reach to count as passing.
    min_validity: float = 1.0


@dataclass
class EvalResult:
    """What one golden run produced and how it scored."""

    case: str
    valid_ids: set[str]
    min_validity: float
    citations: list[str]
    valid: list[str]
    hallucinated: list[str]

    @property
    def rate(self) -> float:
        return citation_validity_rate(self.citations, self.valid_ids)

    @property
    def passed(self) -> bool:
        return self.rate >= self.min_validity

    def assert_valid(self) -> None:
        """Fail the run when the citation-validity rate is below the threshold."""
        if not self.passed:
            raise AssertionError(
                f"eval case {self.case!r}: citation validity {self.rate:.2%} below "
                f"threshold {self.min_validity:.2%}; hallucinated {self.hallucinated!r}"
            )


def evaluate(case: GoldenCase, citations: Iterable[str]) -> EvalResult:
    """Score one golden run, keeping the model's citations and the split."""
    produced = list(citations)
    return EvalResult(
        case=case.name,
        valid_ids=case.evidence_ids,
        min_validity=case.min_validity,
        citations=produced,
        valid=valid_citations(produced, case.evidence_ids),
        hallucinated=hallucinated_citations(produced, case.evidence_ids),
    )