"""Watchlists: domain model, discovery, merging, and the [universe] shim."""

from __future__ import annotations

import pytest

from rigger.core.config import build_config
from rigger.core.ids import make_instrument_id
from rigger.core.models import Instrument
from rigger.core.plugin import discover_watchlists
from rigger.plugins.markets.asx import ASXMarket
from rigger.plugins.watchlists.tickers import TickerWatchlist
from rigger.runtime import Rigger


def _wl(name: str, market: str, tickers: list[str], **extra) -> TickerWatchlist:
    watchlist = TickerWatchlist()
    watchlist.configure({"name": name, "market": market, "tickers": tickers, **extra})
    return watchlist


def test_make_instrument_id():
    assert make_instrument_id("US", "AAPL") == "US:AAPL"
    assert make_instrument_id("ASX", "BHP") == "ASX:BHP"


def test_ticker_watchlist_builds_instruments():
    watchlist = _wl("mining", "asx", ["BHP", "RIO", "FMG"], max_pct=35.0)
    instruments = watchlist.instruments()
    assert [i.id for i in instruments] == ["ASX:BHP", "ASX:RIO", "ASX:FMG"]
    assert all(i.asset_class == "equity" for i in instruments)
    assert all(i.sector is None for i in instruments)
    assert all(i.watchlists == ("mining",) for i in instruments)


def test_ticker_watchlist_asset_class_and_sector_and_tags():
    watchlist = _wl(
        "bonds",
        "us",
        ["TLT"],
        asset_class="bond",
        sector="Government",
        tags=["income", "rates"],
    )
    (instrument,) = watchlist.instruments()
    assert instrument.asset_class == "bond"
    assert instrument.sector == "Government"
    assert instrument.tags == frozenset({"income", "rates"})


def test_ticker_watchlist_currency_defaults_by_market():
    assert _wl("us", "us", ["AAPL"]).instruments()[0].currency == "USD"
    assert _wl("asx", "asx", ["BHP"]).instruments()[0].currency == "AUD"


def test_symbol_overrides():
    watchlist = _wl("mixed", "asx", ["BHP"], overrides={"BHP": {"asset_class": "bond"}})
    (instrument,) = watchlist.instruments()
    assert instrument.asset_class == "bond"


def test_missing_market_raises():
    watchlist = TickerWatchlist()
    watchlist.configure({"name": "nope", "tickers": ["BHP"]})
    with pytest.raises(ValueError, match="must set market"):
        watchlist.instruments()


def test_discover_watchlists_has_tickers_kind():
    kinds = discover_watchlists()
    assert "tickers" in kinds


def test_instrument_defaults_are_stable():
    inst = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")
    assert inst.asset_class == "equity"
    assert inst.watchlists == ()
    assert inst.tags == frozenset()
    assert inst.industry is None
    assert inst.meta == {}


def test_universe_shim_produces_legacy_watchlists():
    cfg = build_config({"universe": {"us": ["AAPL", "MSFT"], "asx": ["BHP"]}})
    assert cfg.watchlists["universe_us"]["tickers"] == ["AAPL", "MSFT"]
    assert cfg.watchlists["universe_asx"]["tickers"] == ["BHP"]
    assert cfg.watchlists["universe_us"]["legacy"] is True


def test_merge_watchlist_into_universe(tmp_path):
    rig = Rigger.__new__(Rigger)
    asx = ASXMarket()
    asx.configure({"tickers": ["BHP", "CBA"]})
    rig.plugins = {"asx": asx}
    legacy = _wl("universe_asx", "asx", ["BHP", "CBA"])
    legacy.name = "universe_asx"
    mining = _wl("mining", "asx", ["BHP", "RIO"])
    rig.watchlists = {"universe_asx": legacy, "mining": mining}

    universe = {i.id: i for i in rig.universe()}
    assert sorted(universe) == ["ASX:BHP", "ASX:CBA", "ASX:RIO"]
    assert set(universe["ASX:BHP"].watchlists) == {"universe_asx", "mining"}
