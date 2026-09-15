"""Scope: declarative filter on DataPlugin and watch targets."""

from __future__ import annotations

from rigger.core.models import Instrument
from rigger.core.plugin import DataPlugin, Scope, parse_scope


def _inst(
    id: str,
    *,
    market: str,
    asset_class: str = "equity",
    watchlists: tuple[str, ...] = (),
    tags: frozenset[str] = frozenset(),
) -> Instrument:
    return Instrument(
        id=id,
        market=market,
        symbol=id.split(":")[-1],
        currency="AUD",
        asset_class=asset_class,
        watchlists=watchlists,
        tags=tags,
    )


def test_default_scope_matches_everything():
    universe = [
        _inst("ASX:BHP", market="asx", watchlists=("mining",)),
        _inst("US:TLT", market="us", asset_class="bond"),
        _inst("US:AAPL", market="us", tags=frozenset({"tech"})),
    ]
    assert Scope().filter(universe) == universe


def test_and_across_axes_or_within_one():
    universe = [
        _inst("ASX:BHP", market="asx", asset_class="equity", watchlists=("mining",)),
        _inst("ASX:RIO", market="asx", asset_class="equity", watchlists=("mining",)),
        _inst("US:TLT", market="us", asset_class="bond", watchlists=("bonds",)),
        _inst("US:AAPL", market="us", asset_class="equity", watchlists=("tech",)),
    ]
    scope = Scope(
        targets=frozenset({"mining", "bonds"}),
        asset_classes=frozenset({"equity"}),
        markets=frozenset({"asx", "us"}),
    )
    filtered = scope.filter(universe)
    assert {i.id for i in filtered} == {"ASX:BHP", "ASX:RIO"}


def test_single_axis_is_or():
    universe = [
        _inst("ASX:BHP", market="asx", watchlists=("mining",)),
        _inst("US:TLT", market="us", watchlists=("bonds",)),
        _inst("US:AAPL", market="us", watchlists=("tech",)),
    ]
    scope = Scope(targets=frozenset({"mining", "tech"}))
    assert {i.id for i in scope.filter(universe)} == {"ASX:BHP", "US:AAPL"}


def test_tags_intersect():
    universe = [
        _inst("US:AAPL", market="us", tags=frozenset({"tech", "large_cap"})),
        _inst("US:MSFT", market="us", tags=frozenset({"tech"})),
        _inst("US:TLT", market="us", tags=frozenset({"income"})),
    ]
    scope = Scope(tags=frozenset({"tech"}))
    assert {i.id for i in scope.filter(universe)} == {"US:AAPL", "US:MSFT"}


def test_parse_scope_folds_market_and_reads_axes():
    scope = parse_scope(
        {"targets": ["mining"], "asset_classes": "equity", "tags": ["x", "y"]},
        market="asx",
    )
    assert scope.targets == frozenset({"mining"})
    assert scope.asset_classes == frozenset({"equity"})
    assert scope.markets == frozenset({"asx"})
    assert scope.tags == frozenset({"x", "y"})


def test_parse_scope_accepts_legacy_watchlists_key():
    scope = parse_scope({"watchlists": ["mining", "bonds"]})
    assert scope.targets == frozenset({"mining", "bonds"})


def test_parse_scope_markets_beat_market_fallback():
    scope = parse_scope({"markets": ["us"]}, market="asx")
    assert scope.markets == frozenset({"us"})


def test_parse_scope_string_is_markets():
    assert parse_scope("us").markets == frozenset({"us"})


def test_parse_scope_empty_is_unrestricted():
    assert parse_scope(None) == Scope()
    assert parse_scope({}) == Scope()


def test_data_plugin_scope_defaults_to_market():
    class _Data(DataPlugin):
        market = "asx"

        async def fetch(self, instruments, since):
            return []

    d = _Data()
    d.configure({})
    assert d.scope.markets == frozenset({"asx"})

    marketless = type("_Any", (DataPlugin,), {"market": None})()
    marketless.configure({})
    assert marketless.scope.markets is None
