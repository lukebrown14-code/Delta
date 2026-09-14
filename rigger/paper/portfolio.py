"""Paper portfolio: cash, positions, realised/unrealised P&L, fee + slippage model."""

from __future__ import annotations

from datetime import UTC, datetime

from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from rigger.core.db import BarTable, CashTable, FillTable, PositionTable
from rigger.core.models import Fill, Order, Position

MARKET_FEES = {
    "us": (0.0, 0.0),  # (bps, min_usd)
    "asx": (10.0, 10.0),  # 0.1% min $10
    "crypto": (10.0, 0.0),  # 0.1%
}


class PaperPortfolio:
    def __init__(
        self,
        engine: Engine,
        base_currency: str,
        slippage_bps: float = 5.0,
    ) -> None:
        self.engine = engine
        self.base_currency = base_currency
        self.slippage_bps = slippage_bps

    def cash(self) -> float:
        with Session(self.engine) as session:
            row = session.get(CashTable, 1)
            return row.balance if row else 0.0

    def positions(self) -> list[Position]:
        with Session(self.engine) as session:
            rows = session.exec(select(PositionTable)).all()
            return [
                Position(
                    instrument_id=r.instrument_id,
                    qty=r.qty,
                    avg_price=r.avg_price,
                    opened_ts=r.opened_ts,
                    signal_id=r.signal_id,
                )
                for r in rows
            ]

    def fill(
        self,
        order: Order,
        market: str,
        fill_price: float | None = None,
    ) -> Fill:
        price = order.limit_price if order.type == "limit" and order.limit_price else fill_price
        if price is None:
            price = self.latest_price(order.instrument_id)
        assert price is not None and price > 0, f"no fill price available for {order.instrument_id}"

        direction = 1 if order.side == "buy" else -1
        slippage = self.slippage_bps / 10000.0 * direction
        executed = price * (1 + slippage)
        fee_bps, fee_min = MARKET_FEES.get(market, (0.0, 0.0))
        fee = executed * order.qty * (fee_bps / 10000.0)
        if fee_min and fee < fee_min:
            fee = fee_min

        with Session(self.engine) as session:
            cash = session.get(CashTable, 1)
            assert cash is not None, "cash row missing"
            pos = session.get(PositionTable, order.instrument_id)
            if order.side == "sell" and (pos is None or pos.qty + 1e-9 < order.qty):
                held = pos.qty if pos is not None else 0.0
                raise ValueError(
                    f"cannot sell {order.qty} {order.instrument_id}: only {held} held "
                    "(short positions are not supported by the paper portfolio)"
                )

            notional = executed * order.qty
            if order.side == "buy":
                cash.balance -= notional + fee
            else:
                cash.balance += notional - fee

            if order.side == "buy":
                if pos is None:
                    session.add(
                        PositionTable(
                            instrument_id=order.instrument_id,
                            qty=order.qty,
                            avg_price=executed,
                            opened_ts=datetime.now(UTC),
                            signal_id=order.signal_id,
                        )
                    )
                else:
                    new_qty = pos.qty + order.qty
                    pos.avg_price = (pos.avg_price * pos.qty + executed * order.qty) / new_qty
                    pos.qty = new_qty
            else:
                if pos is not None:
                    pos.qty -= order.qty
                    if pos.qty <= 1e-9:
                        session.delete(pos)

            fill = FillTable(
                order_id=order.id,
                ts=datetime.now(UTC),
                qty=order.qty,
                price=executed,
                fee=fee,
                slippage=slippage,
            )
            session.add(fill)
            session.commit()

        return Fill(
            order_id=order.id,
            ts=datetime.now(UTC),
            qty=order.qty,
            price=executed,
            fee=fee,
            slippage=slippage,
        )

    def equity(self, prices: dict[str, float] | None = None) -> float:
        total = self.cash()
        for pos in self.positions():
            if prices and pos.instrument_id in prices:
                total += pos.qty * prices[pos.instrument_id]
            else:
                total += pos.qty * pos.avg_price
        return total

    def position_qty(self, instrument_id: str) -> float:
        with Session(self.engine) as session:
            pos = session.get(PositionTable, instrument_id)
            return pos.qty if pos is not None else 0.0

    def latest_price(self, instrument_id: str) -> float | None:
        with Session(self.engine) as session:
            row = session.exec(
                select(BarTable)
                .where(BarTable.instrument_id == instrument_id)
                .order_by(BarTable.ts.desc())
            ).first()
        return row.close if row else None
