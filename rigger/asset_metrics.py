"""Asset-class-aware live metrics for the Watchlist inspector."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any

from rigger.core.models import Instrument
from rigger.plugins.data.yfinance import DEFAULT_SUFFIXES, yf_symbol


@dataclass
class AssetMetrics:
    instrument_id: str
    profile: str
    values: dict[str, str] = field(default_factory=dict)
    groups: dict[str, dict[str, str]] = field(default_factory=dict)
    series: list[float] = field(default_factory=list)
    change_label: str = ""
    fetched_at: datetime | None = None
    error: str | None = None


PROFILES = {"equity", "etf", "commodity", "bond", "fx", "crypto", "cash", "other"}


def profile_for(instrument: Instrument) -> str:
    asset = instrument.asset_class
    if asset in PROFILES:
        return asset
    return "other"


def fetch_asset_metrics(
    instrument: Instrument, suffixes: dict[str, str] | None = None
) -> AssetMetrics:
    """Fetch provider data and normalize it; intended to run off the UI thread."""
    result = AssetMetrics(instrument.id, profile_for(instrument))
    try:
        import yfinance as yf

        ticker = yf.Ticker(yf_symbol(instrument, suffixes or DEFAULT_SUFFIXES))
        info: dict[str, Any] = ticker.info or {}
        history = ticker.history(period="1mo", interval="1d", auto_adjust=True)
        if history is not None and not history.empty:
            closes = [float(value) for value in history["Close"].dropna().tolist()]
            result.series = closes
            if closes:
                result.values["Current yield" if result.profile == "bond" else "Current price"] = (
                    f"{closes[-1]:,.2f}"
                )
            if len(closes) > 1:
                if result.profile == "bond":
                    result.change_label = f"{(closes[-1] - closes[0]) * 100:+.1f} bps"
                else:
                    result.change_label = f"{(closes[-1] / closes[0] - 1) * 100:+.1f}%"
        result.groups = _group_values(result.profile, info)
        current_label = "Current yield" if result.profile == "bond" else "Current price"
        if current_label in result.values:
            group = "Yield & Rate" if result.profile == "bond" else "Market Snapshot"
            result.groups.setdefault(group, {})[current_label] = result.values[current_label]
        result.values.update(
            {label: value for group in result.groups.values() for label, value in group.items()}
        )
        result.fetched_at = datetime.now(UTC)
    except Exception as exc:  # provider failures are displayed in the inspector
        result.error = str(exc)
    return result


def _fmt(value: Any, suffix: str = "") -> str | None:
    if value is None or value == "":
        return None
    try:
        number = float(value)
    except (TypeError, ValueError):
        return str(value)
    if suffix in ("%", "bps") and abs(number) <= 1:
        number *= 100
    return f"{number:.1f}{suffix}"


def _values(profile: str, info: dict[str, Any]) -> dict[str, str]:
    return {
        label: value
        for group in _group_values(profile, info).values()
        for label, value in group.items()
    }


def _group_values(profile: str, info: dict[str, Any]) -> dict[str, dict[str, str]]:
    keys: dict[str, tuple[str, str]]
    groups: tuple[tuple[str, tuple[str, ...]], ...]
    if profile == "equity":
        keys = {
            "Revenue growth": ("revenueGrowth", "%"),
            "EPS growth": ("earningsGrowth", "%"),
            "Operating margin": ("operatingMargins", "%"),
            "Net margin": ("profitMargins", "%"),
            "ROIC": ("returnOnInvestedCapital", "%"),
            "ROE": ("returnOnEquity", "%"),
            "Free cash flow": ("freeCashflow", ""),
            "Debt / EBITDA": ("netDebtToEBITDA", "x"),
            "Interest coverage": ("interestCoverage", "x"),
            "P/E": ("trailingPE", "x"),
            "Forward P/E": ("forwardPE", "x"),
            "EV / EBITDA": ("enterpriseToEbitda", "x"),
            "Dividend yield": ("dividendYield", "%"),
            "Payout ratio": ("payoutRatio", "%"),
        }
        groups = (
            (
                "Profitability",
                ("Revenue growth", "EPS growth", "Operating margin", "Net margin", "ROIC", "ROE"),
            ),
            ("Valuation", ("P/E", "Forward P/E", "EV / EBITDA")),
            ("Balance Sheet", ("Free cash flow", "Debt / EBITDA", "Interest coverage")),
            ("Shareholder Returns", ("Dividend yield", "Payout ratio")),
        )
    elif profile == "etf":
        keys = {
            "Expense ratio": ("annualReportExpenseRatio", "%"),
            "Assets under management": ("totalAssets", ""),
            "Distribution yield": ("yield", "%"),
            "Holdings": ("holdingsCount", ""),
        }
        groups = (
            ("Fund Costs", ("Expense ratio",)),
            ("Fund Scale", ("Assets under management",)),
            ("Fund Structure", ("Holdings",)),
            ("Income", ("Distribution yield",)),
        )
    elif profile == "commodity":
        keys = {
            "Volume": ("volume", ""),
            "Open interest": ("openInterest", ""),
            "Contract": ("contractSize", ""),
        }
        groups = (
            ("Market Activity", ("Volume", "Open interest")),
            ("Contract Details", ("Contract",)),
        )
    elif profile == "bond":
        keys = {
            "Coupon": ("couponRate", "%"),
            "Maturity": ("maturityDate", ""),
            "Duration": ("duration", ""),
            "Credit rating": ("creditRating", ""),
        }
        groups = (
            ("Yield & Rate", ()),
            ("Risk", ("Duration", "Credit rating")),
            ("Bond Terms", ("Coupon", "Maturity")),
        )
    elif profile == "fx":
        keys = {"Bid": ("bid", ""), "Ask": ("ask", ""), "Day range": ("dayRange", "")}
        groups = (("Live Quote", ("Bid", "Ask")), ("Session Range", ("Day range",)))
    elif profile == "crypto":
        keys = {
            "Market cap": ("marketCap", ""),
            "Circulating supply": ("circulatingSupply", ""),
            "Volume": ("volume24Hr", ""),
        }
        groups = (
            ("Market Size", ("Market cap",)),
            ("Activity", ("Volume",)),
            ("Supply", ("Circulating supply",)),
        )
    elif profile == "cash":
        keys = {"Yield": ("yield", "%")}
        groups = (("Income", ("Yield",)), ("Liquidity", ()))
    else:
        keys = {}
        groups = (("Market Snapshot", ()), ("Available Metrics", ()))
    values: dict[str, str] = {}
    for label, (key, suffix) in keys.items():
        formatted = _fmt(info.get(key), suffix)
        if formatted is not None:
            values[label] = formatted
    grouped: dict[str, dict[str, str]] = {}
    for title, labels in groups:
        selected = {label: values[label] for label in labels if label in values}
        if selected:
            grouped[title] = selected
    if not grouped and values:
        grouped["Available Metrics"] = values
    return grouped


def chart_window(series: list[float], days: int = 30) -> list[float]:
    return series[-days:]
