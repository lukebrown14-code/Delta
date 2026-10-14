# R4 — Parity, review, cutover

- **Branch:** `rewrite/r4-cutover`
- **Runs:** serial, after every R3.2 stream merges
- **Checklist:** `docs/rewrite/CUTOVER.md`

## Work

1. **Adversarial review:** one reviewer agent per crate and per screen compares
   Rust with Python for error paths, empty/None cases, timezones, float rounding,
   sort stability, retries/cancellation, and provenance/citation checks.
   Findings go to the stream files.
2. **Full matrix:** every scenario at 80x24, 120x40 and 200x50.
3. **Benchmarks** on the populated seed: `hyperfine 'delta --version'`, launch
   to first frame, RSS (`/usr/bin/time -l`), frame time holding `]`/scrub, 50
   live tickers. Update `BENCHMARKS.md`.
4. **Manual pass** on a copy of the real `data/delta.db`, every panel and modal.
5. **Cutover PR** (after the [gate] steps in `CUTOVER.md`): remove the Python
   app; CI runs Rust gates only; update `agent/AGENTS.md`, `agent/CODEMAP.md`,
   `README.md` for Rust commands.

## Done

- [ ] Every box in `CUTOVER.md` ticked.
