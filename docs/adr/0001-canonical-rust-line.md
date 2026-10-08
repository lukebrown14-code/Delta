# Canonical Rust line and the donor port

The reviewed R3 line on `rewrite/rust` (screen framework, parity-gated services,
task cards) is the canonical base for the rewrite; the big-bang port on
`rewrite/rust-local-port` (all seven screens live, monolithic `main.rs`, weaker
gates) is a donor only. Where both lines implement a service, the base wins; the
port contributes only what the base lacks (`asset_metrics`, `schemas`,
`metrics_profiles`, stronger service tests, the `live-*` goldens); static
screens are rebuilt on the Component framework with the port as reference.

Chosen because review history and parity gates outweigh the port's head start —
two parallel implementations of the same features diverged (17k vs 22k lines),
and wholesale reconciliation would discard the review discipline the base line
was built on. The port branch is deleted after cutover; the Python side is
preserved by the `python-final` tag.
