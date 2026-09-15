# Reports Plan

> One sourced summary per target: the AI synthesises evidence, never invents it.

## Goal

Given a target and its recent evidence, produce a report with: what changed,
bull case, bear case, risks, catalysts, unknowns, and a general sentiment, each
section citing the evidence items it draws on. The report is a synthesis, not a
trade recommendation.

## Model

```python
class Report:
    target_id: str
    as_of: datetime
    prompt_version: str
    summary: str
    bull: list[Claim]     # text + evidence ids
    bear: list[Claim]
    risks: list[Claim]
    catalysts: list[Claim]
    unknowns: list[str]
    sentiment: float       # -1..1
    sentiment_reasons: list[Claim]
```

`Claim(text, evidence_ids)` is the smallest cited unit; a report with an empty
`evidence_ids` on a substantive claim is a bug.

## Prompt

`report_v1.j2`, following the existing rules: facts-only instruction, the
evidence rendered inline, required schema, and an explicit instruction to omit
anything unsupported. `prompt_version` = template filename.

## Surface

`rig report <target>` and a TUI Reports screen. Reports persist to
`reports/<target>/<date>.md` via a shared renderer.

## Tests

`tests/test_reports.py`: `FakeLLM` returns a structured report; every claim's
evidence ids resolve to stored items; empty-evidence reports are rejected and
re-prompted.
