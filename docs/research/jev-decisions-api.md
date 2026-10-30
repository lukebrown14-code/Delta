# Jev Decisions API: question types, inputs, limits, cost

Research for #58 (map #56). Question: what can the Jev decision model (`typesafe/jev-*` via OpenRouter's Decisions API) do, and what does that allow for Delta's three indicator readings (market regime bull/bear, outlook health, buy/hold/sell lean)?

Sources were read on 2026-10-30. **Unverified** marks a claim no primary source confirms.

## Bottom line

- Jev answers typed questions about a `state` you send. It never generates text. There are four question types: Choice, Noul (yes/no), Score (ordered levels) and Box (image, Jev 1.14 only). Answers come back as probabilities plus a derived `confidence`.
- It judges **what the evidence in the state says**. TypeSafe says not to rely on knowledge in the model weights, and it is weak at numbers, dates and counting. Price maths, trend and date logic belong in Delta's code, passed to Jev as named buckets.
- Cost is negligible: US$0.042 per million input tokens, and output is free. A full 8k-token reading costs about US$0.0003.
- TypeSafe quotes about 100 ms latency. An OpenRouter cookbook run took about 170 ms per request across 8 workers.
- Jev returns no explanation and no per-item attribution. "Evidence used" must be the evidence IDs Delta put in the state. Per-item attribution needs one Noul per evidence item.
- The calibration claims are statistical over groups of predictions. Nothing published shows calibration on financial outcomes, so Delta must earn its own track record.
- The current client supports Choice only. It has no retry, no size guard and untyped answers, and it sends criteria in alphabetical order, which interacts with a documented first-option bias.

## What Jev is

- A "System One" structured decision model from TypeSafe. You send a `state` plus one or more typed questions, and you get back typed answers with probabilities, not generated text. It has no reasoning traces or explanations, and it is not a drop-in replacement for a chat model. ([OpenRouter Jev hub](https://openrouter.ai/docs/guides/community/jev.md))
- It is trained with "RLCD" (reinforcement learning for calibrated decisions). The same weights serve every account: no fine-tuning, no LoRA, no training on customer requests. You customise it only through `state`, `instructions` and `criteria`. ([TypeSafe Models](https://docs.typesafe.ai/models.md), [AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer.md))
- English is the primary training language. Other languages work, with lower accuracy. ([TypeSafe State](https://docs.typesafe.ai/concepts/state.md))

### Versions and IDs

| Model ID (OpenRouter) | Released | Notes |
| - | - | - |
| `typesafe/jev-1.13` | 2026-09-18 | What Delta uses (`config.toml` `[llm.routing].sentiment`). Responses name a dated snapshot, such as `typesafe/jev-1.13-20260917`. |
| `typesafe/jev-1.14` | 2026-10-14 | Same price. Adds Box. |
| `~typesafe/jev-latest` | alias | Moves to each new release. Pin a version when thresholds are tuned against it. |

Sources: the [OpenRouter models API](https://openrouter.ai/api/v1/models?output_modalities=decisions) (`created` timestamps and pricing), the [Jev tutorial](https://openrouter.ai/docs/guides/community/jev-tutorial.md) and [TypeSafe Models](https://docs.typesafe.ai/models.md).

The Decisions API is an **alpha** endpoint: `POST https://openrouter.ai/api/alpha/decisions`, tagged `alpha.decisions` in the OpenAPI spec. Expect schema changes. ([API reference](https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-request.md)) As of 2026-10-16, 22 other decision models from 13 makers accept the same request shape. Their probabilities are not comparable to Jev's, so thresholds must be re-tuned per model. ([Jev hub](https://openrouter.ai/docs/guides/community/jev.md#other-decision-models))

## Question types

All questions in one request see the same state. They are evaluated independently and in parallel, and none can see another's answer. ([Tutorial FAQ](https://openrouter.ai/docs/guides/community/jev-tutorial.md), [State](https://docs.typesafe.ai/concepts/state.md))

| Type | Asks | Request `criteria` | Answer fields | Limits |
| - | - | - | - | - |
| `choice` | Which one of these unordered options? | object `{option: description}` (required) | `choice` (required); `probabilities`, `confidence` (optional) | Up to 255 options ([Choice](https://docs.typesafe.ai/primitives/choice.md)) |
| `noul` | Does this condition hold? | optional `{true, false}` | `noul`, the probability of yes. It has no `confidence`. | — |
| `score` | Where on an ordered scale? | array of level descriptions, `minItems: 1` (required) | `score`, the probability-weighted position (0..n-1, can fall between levels); optional `probabilities`, `confidence`, `legend` | Up to 10 levels, each one describable on its own ([Score](https://docs.typesafe.ai/primitives/score.md)) |
| `box` | Where in an image? | none | `box{top,bottom,left,right}`, `confidence` | Jev 1.14 only; 1.13 returns `400`. Not relevant to Delta. |

Schemas come from the [API reference](https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-request.md). `instructions` and each criterion may be a string, an object or an array, so structured guidance such as "covers", "not this" and examples is allowed.

Usage notes from TypeSafe:

- **Score levels are judged one at a time.** The model never sees a level's number or its neighbours, so each level needs a self-contained description. Do not interpolate exact magnitudes from `score`; only threshold it. ([Score](https://docs.typesafe.ai/primitives/score.md), [Jev 1.13 jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md))
- **A Noul value is a probability, not a degree.** Use Score for "how much". Suggested starting thresholds are 0.7 for yes and 0.3 for no. ([Noul](https://docs.typesafe.ai/primitives/noul.md), [How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one.md))
- **Give Choice an escape option** ("other", "unclear") when the listed options might not fit. ([Choice](https://docs.typesafe.ai/primitives/choice.md))

## Input format and size limits

- **`state`:** a string, JSON object or JSON array. TypeSafe recommends an object with named fields. Questions can point at nested values with backticked paths, such as `` `evidence[3].summary` ``. ([State](https://docs.typesafe.ai/concepts/state.md), [How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one.md))
- **Request rules:** UTF-8 JSON with no BOM. At most 64 nesting levels in each of `state`, `instructions` and `criteria`. Finite numbers only (no NaN or 1e400); send large or exact quantities as strings. Body at most 32 MiB, and gzip or zstd is allowed. ([TypeSafe request rules](https://docs.typesafe.ai/api/errors.md))
- **Context limits:**
  - **TypeSafe direct:** 64k tokens per request (state plus all questions) and 32k tokens per question (state plus any one question). Opt-in "long context" raises both to 262k, but it is "slower and less accurate". There is no limit on the number of questions beyond these. ([Models](https://docs.typesafe.ai/models.md#context-length))
  - **Via OpenRouter:** the context length is listed as 262,144 tokens. Requests with "more than about 32,000 bytes of text" run in long-context mode automatically. ([Jev hub FAQ](https://openrouter.ai/docs/guides/community/jev.md#frequently-asked-questions))
  - **Practical cap for Delta:** keep each request's text under about 32 KB (roughly 8k tokens) so it stays in the default, more accurate mode. **Unverified:** whether OpenRouter's byte threshold counts state only or the whole body; the docs say "text".
- **Accuracy falls as the state grows with irrelevant detail.** Filter in code first and send only what the question needs. ([Jev 1.13 jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md#large-state-full-of-irrelevant-detail))

## How evidence should be framed

From the [Jev 1.13 jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md) page (last reviewed 2026-10-02) and the [How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one.md) guide:

1. **Keep arithmetic, counting and date comparison in code.** Jev "is not a calculator", "does not count reliably" and "reads dates as text". Compute returns, moving-average position, drawdown and evidence age in Delta, then pass **named buckets** such as `"trend": "price above its 200-day average for 3 months"` or `"age": "this week"`, not raw series. Jev is weak on numeric representations.
2. **Ask atomic questions.** TypeSafe calls this "probably the most important concept". Decompose a broad judgment such as "is this a bull market?" into narrow Nouls and Scores, then combine them in code with weights or a downstream classical model.
3. **Ask many questions per request.** They run in parallel, so decomposition adds no round trips.
4. **Write literal instructions.** Jev answers the words you wrote. Put boundary cases in `criteria`, and keep `instructions` and `criteria` aligned.
5. **Reorder Choice options to check consistency.** Jev 1.13 "leans toward the option that comes first".
6. **Treat state as data, not commands.** Jev is not hardened against adversarial or self-promoting text in the state. News and press releases argue for themselves, so be explicit in the criteria.
7. **Supply knowledge in the state, not from the weights.** "Do not rely on knowledge stored in model weights when current information can come from your own knowledge base." Jev has no live market knowledge; it judges the evidence Delta sends.

## Output shape and calibration claims

Example response (Choice, Noul and Score in one call), from the [tutorial](https://openrouter.ai/docs/guides/community/jev-tutorial.md#step-3-read-the-answers):

```json
{
  "id": "gen-dec-…", "model": "typesafe/jev-1.13-20260917", "provider": "TypeSafe",
  "answers": {
    "is_bug":  {"type": "noul", "noul": 0.96},
    "team":    {"type": "choice", "choice": "payments", "confidence": 0.67,
                "probabilities": {"payments": 0.78, "frontend": 0.22, "account": 0}},
    "urgency": {"type": "score", "score": 1.99, "confidence": 0.99,
                "probabilities": {"0": 0, "1": 0, "2": 1}, "legend": {"0": "…", "1": "…", "2": "…"}}
  },
  "usage": {"input_tokens": 476, "output_tokens": 70, "cost": 0.000019992}
}
```

- **`confidence` is derived from `probabilities`.** It is 1 when all probability sits on one outcome and 0 when it is spread evenly.
  - For Choice: `(p_max − 1/n)/(1 − 1/n)`.
  - For Score: a distance-weighted version, so a split between neighbouring levels is penalised less than a split between the ends.
  - Noul has no confidence; use `|2p − 1|`.
  ([Confidence](https://docs.typesafe.ai/confidence.md))
- **Probabilities vary slightly between runs.** ([Tutorial](https://openrouter.ai/docs/guides/community/jev-tutorial.md)) TypeSafe also claims answers are "self-consistent" across repeats. ([How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one.md))
- **Calibration claim.** RLCD optimises probabilities "against outcomes": across many predictions, events given 0.8 should happen about 80% of the time. TypeSafe states this holds across groups and "does not guarantee that an individual answer is correct". ([System One](https://docs.typesafe.ai/concepts/system-one.md), [AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer.md))
- **Unverified:** there are no published calibration curves for Jev, and no evaluation on financial text or market outcomes. The calibration describes how well Jev reads the state against the question, not whether a price will rise. Delta's trade-outcome feedback loop (map #56) is the only way to measure lean accuracy.
- **Thresholds come from your own labelled data**, set by the cost of each error type, not by round numbers. The OpenRouter classification cookbook labels 100–200 items. On TweetTopic, Choice confidence ≥0.8 was right 114/122 times; 0.5–0.8 was right 9/18. ([Classification cookbook](https://openrouter.ai/docs/cookbook/evaluate-and-optimize/jev-classification.md))

## Latency

- **TypeSafe:** "Most calls complete in about 100 ms." ([How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one.md#what-makes-system-one-composable)) This is for TypeSafe's own endpoint.
- **Via OpenRouter:** 400 requests of about 610 tokens each took 8.6 s with 8 workers and no retries. That works out to roughly 170 ms per request including network (derived). ([Classification cookbook](https://openrouter.ai/docs/cookbook/evaluate-and-optimize/jev-classification.md))
- **Long context:** long-context mode is "slower"; no figure is given. **Unverified:** latency for 8k–32 KB states via OpenRouter, so measure it before relying on interactive refresh.
- **Delta's setting:** the current client timeout is 30 s (`REQUEST_TIMEOUT`), far above need.

## Pricing

- **Price:** US$0.042 per million input tokens, US$0 for output. This is the same for 1.13, 1.14 and the alias. ([Models API](https://openrouter.ai/api/v1/models?output_modalities=decisions), [TypeSafe Models](https://docs.typesafe.ai/models.md)) Long context costs the same per token.
- **`usage.cost` (USD) comes back on every response.** The tutorial figure checks out: 476 × 0.042e-6 = 0.000019992. The schema marks `cost` as optional, and the cookbook rejects responses that lack it rather than treating them as free.
- **Delta estimates (derived, not measured):**
  - One 8k-token reading request costs about US$0.00034.
  - 50 instruments × 3 readings per day, as separate 8k requests, comes to 150 × 8k = 1.2M tokens, about US$0.05 per day.
  - Per-evidence-item Nouls add only question tokens.
- **Unverified:** whether a multi-question request bills the state once or once per question. TypeSafe says Jev "ingests the `state` once", which suggests once.

## Rate limits and errors

- **TypeSafe** sets limits per organisation, in tokens per second and requests per second, and shows them in its console. They are not published. ([Models](https://docs.typesafe.ai/models.md#rate-limits))
- **OpenRouter:**
  - It publishes no per-model limit for paid models; limits are governed globally and differ by model. ([OpenRouter limits](https://openrouter.ai/docs/api_reference/limits.md))
  - A `429` can come from OpenRouter (DDoS protection) or from the provider. Successful responses carry no `X-RateLimit-*` headers.
  - A `402` whose `limit_source` is `openrouter_in_flight_budget` is transient. It applies to low-balance prepaid accounts, carries `Retry-After`, and should be retried; any other `402` means credits have run out.
- **Unverified:** the effective TypeSafe limit applied to OpenRouter traffic.
- **Recommended handling:** bounded concurrency, starting at 8 workers (the cookbook ran with no 429s). Retry 429, 5xx, 529 (TypeSafe overloaded) and the in-flight-budget 402 with `Retry-After` or capped exponential backoff of 1 s → 30 s. Fail fast on other 4xx: 400 is a rule break or `max_tokens_exceeded`, 413 is a body over 32 MiB, 422 is a malformed question. ([Cookbook](https://openrouter.ai/docs/cookbook/evaluate-and-optimize/jev-classification.md), [TypeSafe errors](https://docs.typesafe.ai/api/errors.md))

## What the existing code does

### `crates/delta-llm/src/jev.rs` (client)

- `JevClient::decide` / `decide_with_model` POSTs `{model, state, questions}` to `/api/alpha/decisions` with a Bearer key and a 30 s timeout.
- **Only `ChoiceQuestion` exists.** There is no Noul or Score builder.
- **Answers are untyped.** `Decision.answers` is a raw `serde_json::Map`. `usage` defaults missing fields to 0, including a missing `cost`, which is treated as free.
- **Caching:** responses are cached by `payload_hash`, which is `stable_id` over Python-compatible sorted JSON of `{model, prompt_version, state, questions}`. A hit is re-logged as a `cached=true` row at zero cost. Because the hash keys on the model *alias* string, a `~typesafe/jev-latest` cache hit could replay an older version's answer. Keep the model pinned.
- Every live call is logged to `llmcall` with tokens, cost, latency, the response model snapshot and the raw body.
- **No retry or backoff.** Any non-200 response becomes `JevError::Api` (the 200-character body snippet is kept). The UI/UX audit flagged this as B14.
- **No state-size guard** against the ~32 KB long-context threshold.
- **Criteria order is lost.** `ChoiceQuestion.criteria` is an ordered `Vec`, but `payload()` collects it into a `serde_json::Map`. The workspace does not enable serde_json's `preserve_order` feature (`Cargo.lock` lists no `indexmap` dependency for `serde_json`), so the map is a `BTreeMap` and options are **sent alphabetically**. The stance question therefore goes out as `bear, bull, neutral`. With Jev 1.13's documented lean toward the first option, this could tilt stances bearish (**unverified** magnitude).
  - Fixing it needs either the `preserve_order` feature (a dependency/feature change, so ask first) or building the request JSON by hand.
  - The hash would be unaffected, because `python_dumps` sorts keys itself.
- `crates/delta-llm/tests/jev.rs` covers parsing, caching, cache-hit logging, 502 error mapping and Python hash parity. It does not cover Noul/Score, retry, or option order.

### `crates/delta-services/src/sentiment.rs` (news stance)

- **One Choice per (news item, instrument)**, bull/bear/neutral. The state holds one item: `instrument_id`, `symbol`, `published`, `source`, `title`, `body` and `evidence_id`.
- **Calls are sequential, one request per item.** A failure skips the item, which the next run retries.
- **Storage:** results go to the `sentiment` table with stance, confidence, a probabilities JSON, the model snapshot and `prompt_version`. Rows are keyed by `stable_id(instrument_id, evidence_id)`, so each stance traces back to one evidence row.
- **Unknown choices read as neutral.** A missing `confidence` is stored as 0, which gives that row zero weight in the summary.
- **`stock_sentiment`** is a weighted score over a window, from −1 to 1. Each row's weight is its confidence times a 7-day recency half-life.
  - It has no caller outside the tests in `crates/` today.
  - It is a natural code-computed input to the regime and lean readings.

### `crates/delta-services/src/pipeline.rs`

- `gather_configured` runs ingest, then extraction, then `classify_sentiment`, using `OPENROUTER_API_KEY` from `.env`. A sentiment failure becomes a warning.
- The module doc comment still says "Sentiment classification (Jev) is not ported yet". That is stale.

## What the three readings need

All three are **indicators, not advice** (map #56). Each is one Jev request per subject. Delta's code builds the state from stored evidence and harness reports, with numbers pre-bucketed. The question set is versioned by `prompt_version`.

| Reading | Subject | Suggested shape | Combine in code |
| - | - | - | - |
| Market regime | Space (its market or index) | **Score**, 5 levels: strong bear / bear / range-bound / bull / strong bull, each described in words. Add **Nouls** for atomic signals: "Is the trend described in `trend` upward?", "Does breadth evidence show broad participation?", "Is volatility described as elevated?" | Score position plus Noul signals plus code-computed trend buckets. Show probabilities and confidence. |
| Outlook health | Instrument (and Space roll-up) | **Score**, 5 levels from "deteriorating sharply" to "improving clearly", over recent reports and evidence. Add **Nouls** per premise or falsifier: "Does any item in `evidence` contradict `premise`?" | Score gated on confidence. Falsifier Nouls ≥0.7 flag the reading. |
| Buy/hold/sell lean | Instrument | **Choice** `{add, hold, reduce}`, or `{buy, hold, sell}` worded as "the evidence on balance supports…", plus an `unclear` escape option. Run a second pass with options reordered to check for first-option bias. | Report the lean only above a tuned confidence threshold; otherwise show "unclear". |

### Evidence used

Jev cannot report which inputs drove an answer. Two ways to get provenance:

- **By construction:** store the list of evidence IDs placed in the state alongside the reading.
- **For attribution:** add one Noul per evidence item, such as "Does `evidence[i]` support a bullish reading of `subject`?". This is TypeSafe's fan-out pattern ([Noul structured instructions](https://docs.typesafe.ai/primitives/noul.md)). It returns per-item support probabilities for the TUI.

### Client and service gaps

1. Add `NoulQuestion` and `ScoreQuestion`, plus typed answer parsing: `ChoiceAnswer`, `NoulAnswer` and `ScoreAnswer`, with optional `confidence` and `probabilities`. Treat a missing `usage.cost` as an error or unknown, not as free.
2. Keep option order as written: use `preserve_order` (ask first, as it is a dependency feature change) or hand-built JSON. Add an order-swap consistency check for the lean.
3. Add retry and backoff for 429, 5xx, 529 and the in-flight-budget 402, honouring `Retry-After`. Add a bounded-concurrency runner.
4. Add a state builder that filters evidence, buckets numbers and dates, and enforces a ~32 KB text budget so requests stay out of long-context mode.
5. Add a readings table, which `sentiment` does not cover. Each row needs: subject (Space or instrument), reading kind, question key, answer type, value, probabilities, confidence, the evidence IDs in the state, model snapshot, prompt version and timestamp. Also store reading-to-trade-outcome links for the track record.
6. Pin `typesafe/jev-1.13`, or move to `1.14` at the same price, deliberately. Re-tune thresholds on a labelled sample whenever the model changes.
7. Label each reading in the UI as an indicator, showing its probabilities and confidence, never as a recommendation.

## Conflicts between sources

- **Image input.** TypeSafe's [System One](https://docs.typesafe.ai/concepts/system-one.md) and [Models](https://docs.typesafe.ai/models.md) pages say Jev takes text only. OpenRouter lists `text+image` for 1.13 and 1.14 and documents image limits ([Multimodal Decisions](https://openrouter.ai/docs/guides/community/multimodal-decisions.md)), calling images an early feature. This does not affect Delta.
- **Context size.** TypeSafe's direct default is 64k per request and 32k tokens per question. OpenRouter states 262,144 tokens, with an automatic long-context switch above about 32,000 *bytes*. Treat 32 KB as the safe ceiling.
