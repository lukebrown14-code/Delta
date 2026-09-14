"""Tests for the ASX market plugin."""

from __future__ import annotations

from datetime import UTC, datetime
from zoneinfo import ZoneInfo

import pytest

from rigger.plugins.markets.asx import ASXMarket

SYDNEY = ZoneInfo("Australia/Sydney")


@pytest.fixture
def asx() -> ASXMarket:
    m = ASXMarket()
    m.configure({"tickers": ["BHP", "CBA", "CSL", "WES", "FMG"]})
    return m


def test_universe_shape(asx: ASXMarket) -> None:
    insts = asx.universe()
    assert [i.id for i in insts] == ["ASX:BHP", "ASX:CBA", "ASX:CSL", "ASX:WES", "ASX:FMG"]
    for inst in insts:
        assert inst.market == "asx"
        assert inst.currency == "AUD"
        assert inst.sector is None
        assert inst.id == f"ASX:{inst.symbol}"


def test_universe_empty_without_config() -> None:
    assert ASXMarket().universe() == []


@pytest.mark.parametrize(
    ("local", "expected"),
    [
        (datetime(2026, 9, 14, 9, 59, tzinfo=SYDNEY), False),  # Monday, pre-open
        (datetime(2026, 9, 14, 10, 0, tzinfo=SYDNEY), True),  # Monday, open bell
        (datetime(2026, 9, 14, 12, 30, tzinfo=SYDNEY), True),  # Monday, midday
        (datetime(2026, 9, 14, 16, 0, tzinfo=SYDNEY), True),  # Monday, close bell
        (datetime(2026, 9, 14, 16, 1, tzinfo=SYDNEY), False),  # Monday, after close
        (datetime(2026, 9, 12, 12, 0, tzinfo=SYDNEY), False),  # Saturday
        (datetime(2026, 9, 13, 12, 0, tzinfo=SYDNEY), False),  # Sunday
    ],
)
def test_is_open(asx: ASXMarket, local: datetime, expected: bool) -> None:
    assert asx.is_open(local) is expected
    # Same instant expressed in UTC must give the same answer.
    assert asx.is_open(local.astimezone(UTC)) is expected


def test_next_open_from_weekend(asx: ASXMarket) -> None:
    saturday = datetime(2026, 9, 12, 12, 0, tzinfo=SYDNEY)
    nxt = asx.next_open(saturday)
    assert nxt.tzinfo == UTC
    assert nxt.astimezone(SYDNEY) == datetime(2026, 9, 14, 10, 0, tzinfo=SYDNEY)


def test_next_open_from_friday_after_close(asx: ASXMarket) -> None:
    friday = datetime(2026, 9, 11, 17, 0, tzinfo=SYDNEY)
    assert asx.next_open(friday).astimezone(SYDNEY) == datetime(2026, 9, 14, 10, 0, tzinfo=SYDNEY)


def test_next_open_same_day_before_open(asx: ASXMarket) -> None:
    monday_early = datetime(2026, 9, 14, 8, 0, tzinfo=SYDNEY)
    assert asx.next_open(monday_early).astimezone(SYDNEY) == datetime(
        2026, 9, 14, 10, 0, tzinfo=SYDNEY
    )


def test_fee_minimum_and_percentage(asx: ASXMarket) -> None:
    assert asx.fee(5_000.0) == 10.0  # 0.1 % = $5 -> floor at $10
    assert asx.fee(10_000.0) == 10.0  # exactly at the minimum
    assert asx.fee(50_000.0) == pytest.approx(50.0)
    assert asx.fee(0.0) == 10.0
