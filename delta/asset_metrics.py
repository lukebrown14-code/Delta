"""Asset-class-aware live metrics for the Watchlist inspector.

One Yahoo ``ticker.info`` payload feeds every profile: the metric tables in
``delta/metrics.toml`` name the labels worth showing and how to render each
value. The same kind of number arrives scaled differently per key (``yield``
0.0473 is a ratio while ``dividendYield`` 0.32 is already a percent), so every
key states its format explicitly rather than guessing from magnitude.
"""

from __future__ import annotations

import time
import tomllib
from dataclasses import dataclass, field
from datetime import UTC, datetime
from importlib.resources import files
from typing import Any

from delta.core.db import BarTable
from delta.core.models import Instrument
from delta.plugins.data.yfinance import DEFAULT_SUFFIXES, yf_symbol

#: ``label -> (info key, format)`` per profile.
KeySpec = dict[str, tuple[str, str]]

#: ``group title -> labels`` in card order.
GroupSpec = tuple[tuple[str, tuple[str, ...]], ...]


@dataclass
class AssetMetrics:
    instrument_id: str
    profile: str
    values: dict[str, str] = field(default_factory=dict)
    groups: dict[str, dict[str, str]] = field(default_factory=dict)
    series: list[float] = field(default_factory=list)
    # ISO timestamps parallel to `series` (same length, same order) so the
    # chart can label its X axis without re-fetching history.
    series_times: list[str] = field(default_factory=list)
    change_label: str = ""
    fetched_at: datetime | None = None
    source: str = "Yahoo Finance"
    history_start: str | None = None
    history_end: str | None = None
    period_high: float | None = None
    period_low: float | None = None
    volatility: float | None = None
    # Raw 52-week spread for the header position bar (K7); kept separate from
    # the formatted "52w high"/"52w low" values so the bar needs no parsing.
    week_52_high: float | None = None
    week_52_low: float | None = None
    error: str | None = None


PROFILES = {"equity", "etf", "commodity", "bond", "fx", "crypto", "cash", "other"}

#: How long (seconds) a successful ``fetch_asset_metrics`` answer is reused
#: (B12): reopening the inspector within the window skips the Yahoo calls.
_CACHE_TTL = 60.0
_metrics_cache: dict[tuple[str, str], tuple[float, AssetMetrics]] = {}


def clear_metrics_cache() -> None:
    """Drop the in-memory fetch cache (tests and explicit refreshes)."""
    _metrics_cache.clear()


def profile_for(instrument: Instrument) -> str:
    asset = instrument.asset_class
    if asset in PROFILES:
        return asset
    return "other"


def _load_metric_tables() -> tuple[dict[str, tuple[KeySpec, GroupSpec]], dict[str, str]]:
    """Load the static metric tables and glossary from the bundled TOML.

    Kept in a data file (C10) so the ~450 lines of per-profile key/group
    tables do not crowd the module. ``tomllib`` preserves the file's key and
    group order, so card order is exactly what the file spells.
    """
    raw = tomllib.loads(files("delta").joinpath("metrics.toml").read_text(encoding="utf-8"))
    tables: dict[str, tuple[KeySpec, GroupSpec]] = {}
    for name, spec in raw["profiles"].items():
        keys: KeySpec = {label: (key, fmt) for label, key, fmt in spec["keys"]}
        groups: GroupSpec = tuple((title, tuple(labels)) for title, labels in spec["groups"])
        tables[str(name)] = (keys, groups)
    return tables, dict(raw.get("help", {}))


_TABLES, METRIC_HELP = _load_metric_tables()

#: The fallback profile for unknown asset classes, from the data file.
OTHER_KEYS, OTHER_GROUPS = _TABLES["other"]


def groups_for(profile: str) -> GroupSpec:
    """The profile's static group table, in card order."""
    table = _TABLES.get(profile)
    return table[1] if table else OTHER_GROUPS


def _compact(value: float) -> str:
    magnitude = abs(value)
    for threshold, suffix in ((1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")):
        if magnitude >= threshold:
            return f"{value / threshold:,.2f}{suffix}"
    if value.is_integer():
        return f"{value:,.0f}"
    return f"{value:,.2f}"


def _money(value: float) -> str:
    sign = "-" if value < 0 else ""
    return f"{sign}${_compact(abs(value))}"


def _as_float(value: Any) -> float | None:
    """Coerce a provider value to float, or None when it is absent/non-numeric."""
    try:
        if value is None or value == "":
            return None
        return float(value)
    except (TypeError, ValueError):
        return None


def _fmt(value: Any, fmt: str) -> str | None:
    if value is None or value == "":
        return None
    if fmt == "text":
        return str(value)
    if fmt == "date":
        try:
            stamp = datetime.fromtimestamp(int(value), tz=UTC)
        except (TypeError, ValueError, OSError, OverflowError):
            return str(value)
        return f"{stamp:%-d %b %Y}"
    try:
        number = float(value)
    except (TypeError, ValueError):
        return str(value)
    if fmt == "ratio":
        return f"{number * 100:.1f}%"
    if fmt == "percent":
        return f"{number:.1f}%"
    if fmt == "x":
        return f"{number:,.1f}x"
    if fmt == "money":
        return _money(number)
    if fmt == "count":
        return _compact(number)
    if fmt == "fx":
        return f"{number:.4f}"
    if fmt == "price":
        return f"{number:,.2f}" if abs(number) >= 10 else f"{number:,.4f}"
    return f"{number:,.2f}"


#: Yahoo ``(period, interval)`` per inspector range (K8). The legacy ``day`` /
#: ``month`` / ``all`` names are kept as aliases so older callers keep working.
_RANGES: dict[str, tuple[str, str]] = {
    "day": ("1d", "5m"),
    "1d": ("1d", "5m"),
    "5d": ("5d", "1d"),
    "month": ("1mo", "1d"),
    "1m": ("1mo", "1d"),
    "6m": ("6mo", "1d"),
    "ytd": ("ytd", "1d"),
    "1y": ("1y", "1d"),
    "all": ("max", "1wk"),
}


def fetch_asset_metrics(
    instrument: Instrument,
    range_name: str = "month",
    engine: Any = None,
    suffixes: dict[str, str] | None = None,
) -> AssetMetrics:
    """Fetch provider data and normalize it, cached briefly (B12).

    Intended to run off the UI thread. A successful answer is reused for a
    short window so reopening the inspector does not repeat the Yahoo calls;
    the entry is keyed by ``(instrument id, range)``, the only inputs that
    vary across a session.
    """
    key = (instrument.id, range_name)
    cached = _metrics_cache.get(key)
    if cached is not None and time.monotonic() - cached[0] < _CACHE_TTL:
        return cached[1]
    now = time.monotonic()
    result = _fetch_asset_metrics(instrument, range_name, engine, suffixes)
    if result.error is None:
        _metrics_cache[key] = (now, result)
    return result


def _fetch_asset_metrics(
    instrument: Instrument,
    range_name: str = "month",
    engine: Any = None,
    suffixes: dict[str, str] | None = None,
) -> AssetMetrics:
    """Provider fetch and normalization; the uncached body of ``fetch_asset_metrics``."""
    result = AssetMetrics(instrument.id, profile_for(instrument))
    try:
        import yfinance as yf

        ticker = yf.Ticker(yf_symbol(instrument, suffixes or DEFAULT_SUFFIXES))
        info: dict[str, Any] = ticker.info or {}
        period, interval = _RANGES.get(range_name, ("1mo", "1d"))
        history = ticker.history(period=period, interval=interval, auto_adjust=True)
        if range_name in ("day", "1d") and (history is None or history.empty):
            history = ticker.history(period="5d", interval="1d", auto_adjust=True)
        closes: list[float] = []
        times: list[str] = []
        if history is not None and not history.empty:
            # Build closes and times in one pass over the dropped-NaN column,
            # so the two lists can never desync.
            clean = history["Close"].dropna()
            for ts, value in zip(clean.index, clean.tolist(), strict=True):
                times.append(str(ts))
                closes.append(float(value))
        elif range_name == "all" and engine is not None:
            # The provider gave nothing: fall back to the locally stored bars
            # rather than showing an empty chart. They must never be merged
            # into a non-empty provider series — provider "max" already spans
            # those dates, and prepending recent bars would corrupt the
            # first-to-last change and the chart's time order.
            from sqlmodel import Session, select

            with Session(engine) as session:
                local = session.exec(
                    select(BarTable.ts, BarTable.close)
                    .where(BarTable.instrument_id == instrument.id)
                    .order_by(BarTable.ts)
                ).all()
            # One loop appends ts and close together, so they stay parallel.
            for ts, close in local:
                if close is None:
                    continue
                times.append(str(ts))
                closes.append(float(close))
        result.series = closes
        result.series_times = times
        if closes:
            if len(closes) > 1:
                returns = [
                    current / previous - 1
                    for previous, current in zip(closes, closes[1:], strict=False)
                    if previous
                ]
                if returns:
                    mean = sum(returns) / len(returns)
                    result.volatility = (
                        sum((value - mean) ** 2 for value in returns) / len(returns)
                    ) ** 0.5
            result.period_high = max(closes)
            result.period_low = min(closes)
            if history is not None and not history.empty and len(history.index):
                result.history_start = str(history.index[0])
                result.history_end = str(history.index[-1])
            result.values["Current yield" if result.profile == "bond" else "Current price"] = (
                f"{closes[-1]:,.2f}"
            )
            if len(closes) > 1:
                if result.profile == "bond":
                    result.change_label = f"{(closes[-1] - closes[0]) * 100:+.1f} bps"
                else:
                    result.change_label = f"{(closes[-1] / closes[0] - 1) * 100:+.1f}%"
        result.groups = _group_values(result.profile, info)
        if result.profile == "equity":
            _merge_estimates(ticker, result.groups)
        _add_position(result.groups, closes, info)
        result.week_52_high = _as_float(info.get("fiftyTwoWeekHigh"))
        result.week_52_low = _as_float(info.get("fiftyTwoWeekLow"))
        result.values.update(
            {label: value for group in result.groups.values() for label, value in group.items()}
        )
        result.fetched_at = datetime.now(UTC)
    except Exception as exc:  # provider failures are displayed in the inspector
        result.error = str(exc)
        if result.series:
            result.values["Current price"] = f"{result.series[-1]:,.2f}"
    return result


def _merge_estimates(ticker: Any, groups: dict[str, dict[str, str]]) -> None:
    """Fold analyst estimates into Analyst View; most endpoints 404 sometimes."""
    try:
        additions: dict[str, str] = {}
        est = ticker.earnings_estimate
        if est is not None and "+1q" in est.index and "avg" in est.columns:
            eps = _fmt(est.loc["+1q", "avg"], "number")
            if eps:
                additions["EPS est (next q)"] = eps
        if est is not None and "+1q" in est.index and "growth" in est.columns:
            growth = _fmt(est.loc["+1q", "growth"], "ratio")
            if growth:
                additions["EPS growth est"] = growth
        rev = ticker.revenue_estimate
        if rev is not None and "0y" in rev.index and "avg" in rev.columns:
            revenue = _fmt(rev.loc["0y", "avg"], "money")
            if revenue:
                additions["Revenue est (fy)"] = revenue
        if additions:
            # Estimates alone are enough for the card, even when the info
            # payload carried no analyst fields at all.
            groups.setdefault("Analyst View", {}).update(additions)
    except Exception:
        pass


def _add_position(
    groups: dict[str, dict[str, str]], closes: list[float], info: dict[str, Any]
) -> None:
    """Distance from the 52-week high, computed from values already fetched."""
    context = groups.get("Price Context")
    high = info.get("fiftyTwoWeekHigh")
    if context is None or not closes or high in (None, ""):
        return
    try:
        offset = (closes[-1] / float(high) - 1) * 100
    except (TypeError, ValueError, ZeroDivisionError):
        return
    context["From 52w high"] = f"{offset:+.1f}%"


def _values(profile: str, info: dict[str, Any]) -> dict[str, str]:
    return {
        label: value
        for group in _group_values(profile, info).values()
        for label, value in group.items()
    }


def _group_values(profile: str, info: dict[str, Any]) -> dict[str, dict[str, str]]:
    table = _TABLES.get(profile)
    keys, groups = table if table else (OTHER_KEYS, OTHER_GROUPS)
    values: dict[str, str] = {}
    for label, (key, fmt) in keys.items():
        formatted = _fmt(info.get(key), fmt)
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


def chart_window(
    series: list[float], days: int | None = 30, times: list[str] | None = None
) -> tuple[list[float], list[str]]:
    """The trailing window of the series, slicing values and timestamps together."""
    window = series if days is None else series[-days:]
    # Times ride along only when complete and aligned with `series`; a partial
    # or absent list would mislabel the chart's X axis, so it is dropped whole.
    if times is None or len(times) != len(series):
        return window, []
    return window, (times if days is None else times[-days:])
