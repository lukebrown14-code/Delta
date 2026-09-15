"""Runtime wiring shared by the CLI, services, and TUI."""

from __future__ import annotations

from sqlmodel import Session

from rigger.core import config as config_mod
from rigger.core.db import SignalTable, ensure_cash, init_engine
from rigger.core.json import to_json
from rigger.core.models import Signal
from rigger.core.plugin import Context, MarketPlugin, ReportPlugin, apply_config, discover_plugins
from rigger.llm.client import build_client
from rigger.paper.portfolio import PaperPortfolio


class Rigger:
    def __init__(self) -> None:
        self.settings, self.cfg = config_mod.load_config()
        self.engine = init_engine(self.cfg.db_path)
        ensure_cash(self.engine, self.cfg.base_currency, self.cfg.paper_starting_cash)

        self.plugins = discover_plugins()
        apply_config(self.plugins, self.cfg.plugins)
        for plugin in self.plugins.values():
            if isinstance(plugin, ReportPlugin):
                table = dict(self.cfg.plugins.get(plugin.name, {}))
                table.setdefault("reports_dir", self.cfg.reports_dir)
                plugin.configure(table)

        for market_name, tickers in self.cfg.universe.items():
            mp = self.plugins.get(market_name)
            if mp is not None:
                mp.configure({"tickers": tickers})

        self.llm = build_client(
            provider=self.cfg.llm_provider,
            engine=self.engine,
            openrouter_api_key=self.settings.openrouter_api_key,
            litellm_proxy_key=self.settings.litellm_proxy_key,
            proxy_base_url=self.cfg.llm_proxy_base_url,
        )
        currencies = {
            name: p.currency
            for name, p in self.plugins.items()
            if isinstance(p, MarketPlugin) and p.enabled
        }
        self.portfolio = PaperPortfolio(
            self.engine,
            self.cfg.base_currency,
            self.cfg.paper_slippage_bps,
            currencies=currencies,
        )
        broker = self.plugins.get("paper")
        if broker is not None:
            broker.bind(self.portfolio)

    def universe(self):
        out = []
        for plugin in self.plugins.values():
            if plugin.enabled and isinstance(plugin, MarketPlugin):
                out.extend(plugin.universe())
        return out

    def context(self, universe=None) -> Context:
        return Context(
            engine=self.engine,
            settings=self.settings,
            config=self.cfg,
            llm=self.llm,
            universe=self.universe() if universe is None else universe,
            plugins=self.plugins,
        )


def _store_signal(session: Session, s: Signal) -> None:
    session.add(
        SignalTable(
            id=s.id,
            ts=s.ts,
            instrument_id=s.instrument_id,
            strategy=s.strategy,
            direction=s.direction,
            conviction=s.conviction,
            horizon_days=s.horizon_days,
            thesis=s.thesis,
            invalidation=s.invalidation,
            evidence_ids=to_json(s.evidence_ids),
            model=s.model,
            prompt_version=s.prompt_version,
            cost_usd=s.cost_usd,
            metadata_=s.metadata,
        )
    )


store_signal = _store_signal

__all__ = ["Rigger", "_store_signal", "store_signal"]
