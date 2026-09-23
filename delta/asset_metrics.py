"""Asset-class-aware live metrics for the Watchlist inspector.

One Yahoo ``ticker.info`` payload feeds every profile: the key tables below
name the labels worth showing and how to render each value. The same kind of
number arrives scaled differently per key (``yield`` 0.0473 is a ratio while
``dividendYield`` 0.32 is already a percent), so every key states its format
explicitly rather than guessing from magnitude.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime
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
    error: str | None = None


PROFILES = {"equity", "etf", "commodity", "bond", "fx", "crypto", "cash", "other"}


def profile_for(instrument: Instrument) -> str:
    asset = instrument.asset_class
    if asset in PROFILES:
        return asset
    return "other"


EQUITY_KEYS: KeySpec = {
    "Revenue growth": ("revenueGrowth", "ratio"),
    "EPS growth": ("earningsGrowth", "ratio"),
    "Gross margin": ("grossMargins", "ratio"),
    "Operating margin": ("operatingMargins", "ratio"),
    "Net margin": ("profitMargins", "ratio"),
    "EBITDA margin": ("ebitdaMargins", "ratio"),
    "ROIC": ("returnOnInvestedCapital", "ratio"),
    "ROE": ("returnOnEquity", "ratio"),
    "EPS (trailing)": ("trailingEps", "number"),
    "EPS (forward)": ("forwardEps", "number"),
    "Revenue / share": ("revenuePerShare", "number"),
    "P/E": ("trailingPE", "x"),
    "Forward P/E": ("forwardPE", "x"),
    "PEG ratio": ("trailingPegRatio", "x"),
    "Price / Book": ("priceToBook", "x"),
    "Price / Sales": ("priceToSalesTrailing12Months", "x"),
    "EV / EBITDA": ("enterpriseToEbitda", "x"),
    "Free cash flow": ("freeCashflow", "money"),
    "Operating cash flow": ("operatingCashflow", "money"),
    "Total cash": ("totalCash", "money"),
    "Total debt": ("totalDebt", "money"),
    "Debt / EBITDA": ("netDebtToEBITDA", "x"),
    "Interest coverage": ("interestCoverage", "x"),
    "Current ratio": ("currentRatio", "x"),
    "Quick ratio": ("quickRatio", "x"),
    "Debt / Equity": ("debtToEquity", "percent"),
    "Dividend yield": ("dividendYield", "percent"),
    "Dividend rate": ("dividendRate", "number"),
    "Payout ratio": ("payoutRatio", "ratio"),
    "5y avg yield": ("fiveYearAvgDividendYield", "percent"),
    "Market cap": ("marketCap", "money"),
    "Enterprise value": ("enterpriseValue", "money"),
    "Shares out": ("sharesOutstanding", "count"),
    "Float": ("floatShares", "count"),
    "Avg volume": ("averageVolume", "count"),
    "Beta": ("beta", "number"),
    "Short % of float": ("shortPercentOfFloat", "ratio"),
    "Short ratio": ("shortRatio", "x"),
    "Institutions held": ("heldPercentInstitutions", "ratio"),
    "Insiders held": ("heldPercentInsiders", "ratio"),
    "Consensus": ("recommendationKey", "text"),
    "Target mean": ("targetMeanPrice", "price"),
    "Target median": ("targetMedianPrice", "price"),
    "Target high": ("targetHighPrice", "price"),
    "Target low": ("targetLowPrice", "price"),
    "Analysts": ("numberOfAnalystOpinions", "count"),
    "52w high": ("fiftyTwoWeekHigh", "price"),
    "52w low": ("fiftyTwoWeekLow", "price"),
    "52w change": ("52WeekChange", "ratio"),
    "S&P 52w change": ("SandP52WeekChange", "ratio"),
    "50-day average": ("fiftyDayAverage", "price"),
    "200-day average": ("twoHundredDayAverage", "price"),
    "Next earnings": ("earningsTimestamp", "date"),
    "Ex-dividend": ("exDividendDate", "date"),
}

EQUITY_GROUPS: GroupSpec = (
    (
        "Profitability",
        (
            "Revenue growth",
            "EPS growth",
            "Gross margin",
            "Operating margin",
            "Net margin",
            "EBITDA margin",
            "ROIC",
            "ROE",
        ),
    ),
    (
        "Balance Sheet",
        (
            "Free cash flow",
            "Operating cash flow",
            "Total cash",
            "Total debt",
            "Debt / EBITDA",
            "Interest coverage",
            "Current ratio",
            "Quick ratio",
            "Debt / Equity",
        ),
    ),
    (
        "Valuation",
        (
            "P/E",
            "Forward P/E",
            "PEG ratio",
            "Price / Book",
            "Price / Sales",
            "EV / EBITDA",
            "EPS (trailing)",
            "EPS (forward)",
            "Revenue / share",
        ),
    ),
    (
        "Analyst View",
        ("Consensus", "Target mean", "Target median", "Target high", "Target low", "Analysts", "Next earnings"),
    ),
    (
        "Price Context",
        ("52w high", "52w low", "52w change", "S&P 52w change", "50-day average", "200-day average"),
    ),
    ("Shareholder Returns", ("Dividend yield", "Dividend rate", "Payout ratio", "5y avg yield", "Ex-dividend")),
    ("Size", ("Market cap", "Enterprise value", "Shares out", "Float", "Avg volume")),
    ("Trading & Ownership", ("Beta", "Short % of float", "Short ratio", "Institutions held", "Insiders held")),
)

ETF_KEYS: KeySpec = {
    "Category": ("category", "text"),
    "Fund family": ("fundFamily", "text"),
    "NAV": ("navPrice", "price"),
    "Expense ratio": ("annualReportExpenseRatio", "ratio"),
    "Distribution yield": ("yield", "ratio"),
    "3y return": ("threeYearAverageReturn", "ratio"),
    "5y return": ("fiveYearAverageReturn", "ratio"),
    "Assets under management": ("totalAssets", "money"),
    "Holdings": ("holdingsCount", "count"),
    "Previous close": ("previousClose", "price"),
    "Open": ("open", "price"),
    "Day high": ("dayHigh", "price"),
    "Day low": ("dayLow", "price"),
    "52w high": ("fiftyTwoWeekHigh", "price"),
    "52w low": ("fiftyTwoWeekLow", "price"),
    "Volume": ("volume", "count"),
    "Avg volume": ("averageVolume", "count"),
    "50-day average": ("fiftyDayAverage", "price"),
    "200-day average": ("twoHundredDayAverage", "price"),
}

ETF_GROUPS: GroupSpec = (
    ("Fund Info", ("Category", "Fund family", "NAV")),
    ("Fund Costs", ("Expense ratio",)),
    ("Income", ("Distribution yield",)),
    ("Returns", ("3y return", "5y return")),
    ("Fund Scale", ("Assets under management", "Holdings")),
    (
        "Price Context",
        (
            "Previous close",
            "Open",
            "Day high",
            "Day low",
            "52w high",
            "52w low",
            "Volume",
            "Avg volume",
            "50-day average",
            "200-day average",
        ),
    ),
)

BOND_KEYS: KeySpec = {
    "Coupon": ("couponRate", "ratio"),
    "Maturity": ("maturityDate", "date"),
    "Duration": ("duration", "number"),
    "Credit rating": ("creditRating", "text"),
    "Distribution yield": ("yield", "ratio"),
    "Category": ("category", "text"),
    "NAV": ("navPrice", "price"),
    "Assets under management": ("totalAssets", "money"),
    "Previous close": ("previousClose", "price"),
    "Open": ("open", "price"),
    "Day high": ("dayHigh", "price"),
    "Day low": ("dayLow", "price"),
    "52w high": ("fiftyTwoWeekHigh", "price"),
    "52w low": ("fiftyTwoWeekLow", "price"),
    "Volume": ("volume", "count"),
    "Avg volume": ("averageVolume", "count"),
    "50-day average": ("fiftyDayAverage", "price"),
    "200-day average": ("twoHundredDayAverage", "price"),
}

BOND_GROUPS: GroupSpec = (
    ("Income", ("Distribution yield",)),
    ("Fund Scale", ("Assets under management", "NAV", "Category")),
    ("Bond Terms", ("Coupon", "Maturity", "Duration", "Credit rating")),
    (
        "Price Context",
        (
            "Previous close",
            "Open",
            "Day high",
            "Day low",
            "52w high",
            "52w low",
            "Volume",
            "Avg volume",
            "50-day average",
            "200-day average",
        ),
    ),
)

COMMODITY_KEYS: KeySpec = {
    "Volume": ("volume", "count"),
    "Avg volume": ("averageVolume", "count"),
    "Open interest": ("openInterest", "count"),
    "Underlying": ("underlyingSymbol", "text"),
    "Expires": ("expireDate", "date"),
    "Open": ("open", "price"),
    "Day high": ("dayHigh", "price"),
    "Day low": ("dayLow", "price"),
    "Previous close": ("previousClose", "price"),
    "52w high": ("fiftyTwoWeekHigh", "price"),
    "52w low": ("fiftyTwoWeekLow", "price"),
    "50-day average": ("fiftyDayAverage", "price"),
    "200-day average": ("twoHundredDayAverage", "price"),
}

COMMODITY_GROUPS: GroupSpec = (
    ("Market Activity", ("Volume", "Avg volume", "Open interest")),
    ("Contract", ("Underlying", "Expires")),
    (
        "Price Context",
        (
            "Open",
            "Day high",
            "Day low",
            "Previous close",
            "52w high",
            "52w low",
            "50-day average",
            "200-day average",
        ),
    ),
)

FX_KEYS: KeySpec = {
    "Bid": ("bid", "fx"),
    "Ask": ("ask", "fx"),
    "Open": ("open", "fx"),
    "Day high": ("dayHigh", "fx"),
    "Day low": ("dayLow", "fx"),
    "Previous close": ("previousClose", "fx"),
    "52w high": ("fiftyTwoWeekHigh", "fx"),
    "52w low": ("fiftyTwoWeekLow", "fx"),
    "50-day average": ("fiftyDayAverage", "fx"),
    "200-day average": ("twoHundredDayAverage", "fx"),
}

FX_GROUPS: GroupSpec = (
    ("Live Quote", ("Bid", "Ask")),
    ("Session Range", ("Open", "Day high", "Day low", "Previous close")),
    ("Price Context", ("52w high", "52w low", "50-day average", "200-day average")),
)

CRYPTO_KEYS: KeySpec = {
    "Market cap": ("marketCap", "money"),
    "Circulating supply": ("circulatingSupply", "count"),
    "24h volume": ("volume24Hr", "money"),
    "Avg volume": ("averageVolume", "money"),
    "Open": ("open", "price"),
    "Day high": ("dayHigh", "price"),
    "Day low": ("dayLow", "price"),
    "Previous close": ("previousClose", "price"),
    "52w high": ("fiftyTwoWeekHigh", "price"),
    "52w low": ("fiftyTwoWeekLow", "price"),
}

CRYPTO_GROUPS: GroupSpec = (
    ("Market Size", ("Market cap", "Circulating supply")),
    ("Activity", ("24h volume", "Avg volume")),
    (
        "Price Context",
        ("Open", "Day high", "Day low", "Previous close", "52w high", "52w low"),
    ),
)

CASH_KEYS: KeySpec = {
    "Yield": ("yield", "ratio"),
    "Dividend yield": ("dividendYield", "percent"),
    "Category": ("category", "text"),
    "NAV": ("navPrice", "price"),
    "Assets under management": ("totalAssets", "money"),
    "Previous close": ("previousClose", "price"),
    "52w high": ("fiftyTwoWeekHigh", "price"),
    "52w low": ("fiftyTwoWeekLow", "price"),
}

CASH_GROUPS: GroupSpec = (
    ("Income", ("Yield", "Dividend yield")),
    ("Fund Scale", ("Assets under management", "NAV", "Category")),
    ("Price Context", ("Previous close", "52w high", "52w low")),
)

OTHER_KEYS: KeySpec = {
    "Previous close": ("previousClose", "price"),
    "Open": ("open", "price"),
    "Day high": ("dayHigh", "price"),
    "Day low": ("dayLow", "price"),
    "52w high": ("fiftyTwoWeekHigh", "price"),
    "52w low": ("fiftyTwoWeekLow", "price"),
    "Volume": ("volume", "count"),
    "Avg volume": ("averageVolume", "count"),
}

OTHER_GROUPS: GroupSpec = (
    (
        "Price Context",
        (
            "Previous close",
            "Open",
            "Day high",
            "Day low",
            "52w high",
            "52w low",
            "Volume",
            "Avg volume",
        ),
    ),
)

_TABLES: dict[str, tuple[KeySpec, GroupSpec]] = {
    "equity": (EQUITY_KEYS, EQUITY_GROUPS),
    "etf": (ETF_KEYS, ETF_GROUPS),
    "bond": (BOND_KEYS, BOND_GROUPS),
    "commodity": (COMMODITY_KEYS, COMMODITY_GROUPS),
    "fx": (FX_KEYS, FX_GROUPS),
    "crypto": (CRYPTO_KEYS, CRYPTO_GROUPS),
    "cash": (CASH_KEYS, CASH_GROUPS),
}


def groups_for(profile: str) -> GroupSpec:
    """The profile's static group table, in card order."""
    table = _TABLES.get(profile)
    return table[1] if table else OTHER_GROUPS


METRIC_HELP: dict[str, str] = {
    # Profitability
    "Revenue growth": "How fast sales grew in the most recent year.",
    "EPS growth": "How fast profit per share grew in the most recent year.",
    "Gross margin": "Profit left after making the product, per dollar of sales.",
    "Operating margin": "Profit from core operations, per dollar of sales.",
    "Net margin": "Final profit after every expense, per dollar of sales.",
    "EBITDA margin": "Operating profit before accounting charges, per dollar of sales.",
    "ROIC": "Profit made per dollar invested into the business.",
    "ROE": "Profit made per dollar of shareholders' money.",
    "EPS (trailing)": "Profit per share over the past year.",
    "EPS (forward)": "Expected profit per share for the coming year.",
    "Revenue / share": "Sales divided by shares outstanding.",
    # Valuation
    "P/E": "Price per dollar of past-year profit.",
    "Forward P/E": "Price per dollar of expected profit.",
    "PEG ratio": "P/E relative to growth; near 1 reads as fairly priced.",
    "Price / Book": "Price per dollar of accounting net worth.",
    "Price / Sales": "Price per dollar of sales.",
    "EV / EBITDA": "Whole-company price per dollar of operating profit.",
    # Balance sheet
    "Free cash flow": "Cash left after running and growing the business.",
    "Operating cash flow": "Cash the business generated from operations.",
    "Total cash": "Cash and short-term investments held.",
    "Total debt": "All money owed.",
    "Debt / EBITDA": "Years of operating profit needed to repay all debt.",
    "Interest coverage": "How easily profit covers interest payments.",
    "Current ratio": "Ability to pay bills due within a year.",
    "Quick ratio": "Ability to pay bills due within a year, excluding inventory.",
    "Debt / Equity": "How much of the company is funded by borrowing.",
    # Shareholder returns
    "Dividend yield": "Yearly dividend as a percent of the price.",
    "Dividend rate": "Dividend paid per share per year.",
    "Payout ratio": "Share of profit paid out as dividends.",
    "5y avg yield": "Average dividend yield over five years.",
    # Size & liquidity
    "Market cap": "Total value of all shares or coins outstanding.",
    "Enterprise value": "Price to buy the whole company including its debt.",
    "Shares out": "All shares issued.",
    "Float": "Shares freely tradable by the public.",
    "Avg volume": "Typical number of shares traded daily.",
    "Beta": "Sensitivity to market swings; 1 moves with the market.",
    "Short % of float": "Share of tradable shares sold short.",
    "Short ratio": "Days of typical volume needed to cover short positions.",
    "Institutions held": "Share owned by professional funds.",
    "Insiders held": "Share owned by company insiders.",
    # Analyst view
    "Consensus": "Most common analyst rating: buy, hold, or sell.",
    "Target mean": "Average analyst price forecast.",
    "Target median": "Middle analyst price forecast.",
    "Target high": "Highest analyst price forecast.",
    "Target low": "Lowest analyst price forecast.",
    "Analysts": "Number of analysts offering forecasts.",
    "EPS est (next q)": "Analyst forecast for next quarter's profit per share.",
    "EPS growth est": "Forecast growth in profit per share for this year.",
    "Revenue est (fy)": "Analyst forecast for this year's sales.",
    # Price context
    "52w high": "Highest price over the past year.",
    "52w low": "Lowest price over the past year.",
    "52w change": "Price change over the past year.",
    "S&P 52w change": "The S&P 500's change over the same period.",
    "50-day average": "Average price over the last 50 trading days.",
    "200-day average": "Average price over the last 200 trading days.",
    "From 52w high": "How far the current price sits below its yearly high.",
    "Previous close": "Yesterday's official closing price.",
    "Open": "The first traded price of the session.",
    "Day high": "Highest price traded so far today.",
    "Day low": "Lowest price traded so far today.",
    "Volume": "Shares or contracts traded recently.",
    "Current price": "Most recent traded price.",
    "Current yield": "Yearly income as a percent of the current price.",
    # Calendar
    "Next earnings": "When the next results report is due.",
    "Ex-dividend": "Buy before this date to receive the next dividend.",
    # Fund profiles
    "Category": "What kind of product this is.",
    "Fund family": "Company that runs the fund.",
    "NAV": "Per-unit value of the fund's holdings.",
    "Expense ratio": "Yearly fee as a percent of assets.",
    "Distribution yield": "Yearly payouts as a percent of the price.",
    "3y return": "Average yearly return over three years.",
    "5y return": "Average yearly return over five years.",
    "Assets under management": "Total money invested in the fund.",
    "Holdings": "Number of securities the fund holds.",
    # Bond terms
    "Coupon": "Fixed interest the bond pays each year.",
    "Maturity": "When the bond repays its face value.",
    "Duration": "Price sensitivity to interest-rate moves, in years.",
    "Credit rating": "Grading of the issuer's default risk.",
    # Commodity contract
    "Open interest": "Number of outstanding contracts.",
    "Underlying": "The exchange symbol of this contract.",
    "Expires": "When the contract settles.",
    # FX
    "Bid": "Price buyers are offering.",
    "Ask": "Price sellers are asking.",
    # Crypto / cash
    "Circulating supply": "Coins in public circulation.",
    "24h volume": "Value traded in the last day.",
    "Yield": "Yearly interest as a percent of the price.",
}


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


def fetch_asset_metrics(
    instrument: Instrument,
    range_name: str = "month",
    engine: Any = None,
    suffixes: dict[str, str] | None = None,
) -> AssetMetrics:
    """Fetch provider data and normalize it; intended to run off the UI thread."""
    result = AssetMetrics(instrument.id, profile_for(instrument))
    try:
        import yfinance as yf

        ticker = yf.Ticker(yf_symbol(instrument, suffixes or DEFAULT_SUFFIXES))
        info: dict[str, Any] = ticker.info or {}
        period, interval = {
            "day": ("1d", "5m"),
            "month": ("1mo", "1d"),
            "all": ("max", "1wk"),
        }.get(range_name, ("1mo", "1d"))
        history = ticker.history(period=period, interval=interval, auto_adjust=True)
        if range_name == "day" and (history is None or history.empty):
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
