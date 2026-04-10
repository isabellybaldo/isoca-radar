from fastapi import APIRouter, Depends, Query
from sqlalchemy import select, desc
from sqlalchemy.ext.asyncio import AsyncSession

from app import cache
from app.db import get_db
from app.models.trader import FetchLog, Trader

router = APIRouter()

VALID_SORT_FIELDS = {
    "total_profit_usd",
    "roi",
    "win_rate",
    "num_bets",
    "total_volume_usd",
    "consistency_score",
    "politics_bets",
    "finance_bets",
}


@router.get("/traders")
async def list_traders(
    category: str | None = Query(None, description="Filter: politics | finance"),
    sort_by: str = Query("total_profit_usd", description="Field to sort by"),
    limit: int = Query(100, le=200),
    db: AsyncSession = Depends(get_db),
):
    cache_key_suffix = f"{category}:{sort_by}:{limit}"

    # Only use Redis cache for the default (no filters) call — category/sort combos
    # are cheap DB queries so we skip caching those
    if category is None and sort_by == "total_profit_usd" and limit == 100:
        cached = await cache.get_traders_cache()
        if cached is not None:
            return {"traders": cached, "from_cache": True}

    sort_field = sort_by if sort_by in VALID_SORT_FIELDS else "total_profit_usd"

    stmt = select(Trader)

    if category == "politics":
        stmt = stmt.where(Trader.politics_bets > 0)
    elif category == "finance":
        stmt = stmt.where(Trader.finance_bets > 0)

    stmt = stmt.order_by(desc(getattr(Trader, sort_field))).limit(limit)
    result = await db.execute(stmt)
    traders = result.scalars().all()

    data = [
        {
            "address": t.address,
            "username": t.username,
            "total_profit_usd": t.total_profit_usd,
            "total_volume_usd": t.total_volume_usd,
            "num_bets": t.num_bets,
            "win_rate": round(t.win_rate * 100, 1) if t.win_rate is not None else None,
            "roi": round(t.roi * 100, 2),
            "avg_position_usd": round(t.avg_position_usd, 2),
            "politics_bets": t.politics_bets,
            "politics_volume_usd": round(t.politics_volume_usd, 2),
            "finance_bets": t.finance_bets,
            "finance_volume_usd": round(t.finance_volume_usd, 2),
            "consistency_score": round(t.consistency_score * 100, 1) if t.consistency_score is not None else None,
            "last_updated": t.last_updated.isoformat() if t.last_updated else None,
        }
        for t in traders
    ]

    if category is None and sort_by == "total_profit_usd" and limit == 100:
        await cache.set_traders_cache(data)

    return {"traders": data, "from_cache": False}


@router.get("/traders/status")
async def fetch_status(db: AsyncSession = Depends(get_db)):
    """Return the most recent fetch log entry so the UI can show 'last updated'."""
    stmt = select(FetchLog).order_by(desc(FetchLog.id)).limit(1)
    result = await db.execute(stmt)
    log = result.scalar_one_or_none()

    if not log:
        return {"status": "never_run", "last_fetch": None}

    return {
        "status": log.status,
        "started_at": log.started_at.isoformat() if log.started_at else None,
        "finished_at": log.finished_at.isoformat() if log.finished_at else None,
        "traders_discovered": log.traders_discovered,
        "traders_updated": log.traders_updated,
        "error": log.error,
    }
