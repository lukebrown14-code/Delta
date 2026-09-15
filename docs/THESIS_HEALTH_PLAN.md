# Thesis Health Plan

> A deterministic, evidence-based read on how a thesis is faring — not a truth claim.

## Goal

A visual indicator at the top of the thesis panel, plus an AI summary, that says
whether the evidence is accumulating for or against the thesis without ever
pretending the thesis is "right" (it is a forward-looking claim).

## States (deterministic)

| State | Meaning |
|---|---|
| Emerging | just created, little evidence |
| Building | evidence accumulating, no falsifier hit |
| Mixed | support and counter roughly balance |
| Weakening | counter rising, or a partial falsifier |
| Challenged | a falsifier condition met, needs revision |
| Idle | no fresh evidence |

## Computation

A pure function over **accepted** thesis evidence, not the LLM:

1. **Balance** — weight support vs against by recency and (where present)
   source strength; produce a signed tilt in `[-1, 1]`.
2. **Falsifiers** — if any declared falsifier has fired (matched by tag/event),
   state is at least `Weakening`, and `Challenged` if definitively met.
3. **Coverage** — fraction of the thesis scope that has evidence; below a
   threshold forces `Emerging` regardless of tilt.
4. **Freshness** — no accepted evidence newer than N days forces `Idle`.

Output: a state, a tilt value, the counts, and the top evidence ids that drove
the result. The AI then writes one summary paragraph citing those ids.

## Surface

Shown at the top of the Theses panel as a coloured badge + summary; no new
screen.

## Tests

`tests/test_thesis_health.py`: table-driven — each state; falsifier flips a
`Building` thesis to `Challenged`; low coverage forces `Emerging`; stale forces
`Idle`.
