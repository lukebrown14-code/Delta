# Evidence Plan

> Collect, normalise, store, and cite what the harness knows about a target.

## Goal

Every target accumulates a stream of sourced evidence: price bars, news,
filings, fundamentals, calendar events, and — optionally — web hits from an
explicit search. Reports and chat both read this single pool; nothing is
synthesised from memory.

## Model

```python
class EvidenceItem:
    id: str
    target_ids: tuple[str, ...]
    ts: datetime
    kind: Literal["bar", "news", "filing", "fundamental", "event", "web", "note"]
    title: str
    body: str | None
    source: str          # data plugin or "web:<query>"
    url: str | None
    sentiment: float | None   # -1..1 where meaningful
    raw: dict              # plugin-specific payload
```

Existing tables (`bar`, `newsitem`, `fundamental`, `event`) become sources that
feed a unified read path rather than being renamed. The unified `EvidenceItem`
is a read model over them, not a new write table, so existing data plugins keep
writing what they already write.

## Changes

- `services.evidence(engine, *, target=None, since=None, kind=None, limit=...)`
  returns `EvidenceItem` rows across all source tables.
- Grouping by target id, with a stable per-item id and cite text
  (`[source] title <url>`).
- Web search is *not* part of evidence collection here; it is an explicit chat
  tool that returns labelled `Web` items (see Chat plan).

## Tests

`tests/test_evidence.py`: mixed sources unify into `EvidenceItem`; target
filtering; `since` and `kind` filters; cite text is deterministic.
