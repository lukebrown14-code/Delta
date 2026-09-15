# Theses Plan

> An optional long-horizon idea the user wants researched for and against.

## Goal

"Solar panels will meaningfully grow as a share of electricity over the next 10
years." The user creates a thesis; the harness researches it, and the panel shows
supporting evidence, counter-evidence, unknowns, confidence, and what would
falsify it. Theses are optional and never interfere with plain report browsing.

## Model

```python
class Thesis:
    id: str
    claim: str
    scope: str
    assumptions: list[str]
    targets: tuple[str, ...]
    time_horizon: str
    created_at: datetime
    status: Literal["active", "paused", "concluded"]

class ThesisEvidence:
    thesis_id: str
    evidence_id: str
    side: Literal["support", "against", "neutral"]
    note: str            # why it counts, written by the AI or user
    accepted: bool       # candidate until the user confirms
```

## Behaviour

- Discovery is automatic: the AI periodically searches for news related to the
  thesis and proposes **candidate evidence** with a `side` and a note.
- The user accepts, rejects, tags, or annotates candidates. Only accepted
  evidence feeds thesis health.
- The AI maintains a running summary: current view, strongest support,
  strongest counter, unknowns, and falsifiers — always citing accepted evidence.

## Surface

`rig thesis create/list/show`, TUI Theses screen. `evidence_ids` resolve against
the unified evidence pool.

## Tests

`tests/test_theses.py`: create/list/show round-trip; candidate vs accepted;
discovery proposes candidates but does not auto-accept.
