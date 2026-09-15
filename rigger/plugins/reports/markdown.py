"""Markdown report plugin."""

from __future__ import annotations

from pathlib import Path

from rigger.core.plugin import Report, ReportPlugin


class MarkdownReport(ReportPlugin):
    name = "markdown"

    def __init__(self) -> None:
        self._reports_dir = Path("reports")

    def configure(self, cfg: dict) -> None:
        if cfg.get("reports_dir"):
            self._reports_dir = Path(cfg["reports_dir"])

    def render(self, report: Report) -> Path:
        self._reports_dir.mkdir(parents=True, exist_ok=True)
        path = self._reports_dir / f"{report.date}.md"

        lines: list[str] = [
            f"# Rigger Report — {report.date}",
            "",
            f"**Cash:** {report.cash:,.2f} {report.base_currency}",
            *(
                [f"**Equity:** {report.equity:,.2f} {report.base_currency}"]
                if report.equity is not None
                else []
            ),
            "",
            "## Signals",
            "",
        ]
        if not report.signals:
            lines.append("_No signals generated._")
        for s in report.signals:
            lines += [
                f"### {s.instrument_id} — {s.direction.upper()} (conviction {s.conviction:.2f})",
                "",
                f"- **Strategy:** {s.strategy}",
                f"- **Model:** {s.model or 'n/a'}",
                f"- **Horizon:** {s.horizon_days} days",
                f"- **Evidence:** {', '.join(s.evidence_ids) or 'none'}",
                "",
                f"**Thesis:** {s.thesis}",
                "",
                f"**Invalidation:** {s.invalidation}",
                "",
            ]

        lines += ["## Orders", ""]
        if not report.orders:
            lines.append("_No orders._")
        for o in report.orders:
            lines.append(f"- {o.side.upper()} {o.qty:.4f} {o.instrument_id} ({o.type})")
        lines.append("")

        lines += ["## Fills", ""]
        if not report.fills:
            lines.append("_No fills._")
        for f in report.fills:
            lines.append(
                f"- {f.qty:.4f} @ {f.price:.2f} (fee {f.fee:.2f}, slippage {f.slippage:.4f})"
            )
        lines.append("")

        lines += ["## Positions", ""]
        if not report.positions:
            lines.append("_No open positions._")
        for p in report.positions:
            lines.append(f"- {p.instrument_id}: {p.qty:.4f} @ {p.avg_price:.2f}")
        lines.append("")

        path.write_text("\n".join(lines), encoding="utf-8")
        return path
