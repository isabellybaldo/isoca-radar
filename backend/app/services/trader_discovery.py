"""
Trader auto-discovery and metrics computation.

Flow (runs every N hours via asyncio background task):
  1. Fetch leaderboard top N from data-api.polymarket.com
  2. For each address, fetch trade history (with rate-limit sleep between calls)
  3. Compute metrics: ROI, win rate, category breakdown, consistency score
  4. Upsert Trader rows into Postgres
  5. Write FetchLog row
  6. Invalidate Redis cache
"""

import asyncio
import logging
from datetime import datetime, timezone

from sqlalchemy import select, update
from sqlalchemy.dialects.postgresql import insert

from app import cache
from app.config import settings
from app.db import AsyncSessionLocal
from app.models.trader import FetchLog, Trader
from app.services import data_api_client

logger = logging.getLogger(__name__)

# Seconds to sleep between per-trader activity fetches to respect rate limits
_INTER_REQUEST_DELAY = 1.0


def _compute_metrics(
    leaderboard_entry: dict,
    trades: list[dict],
) -> dict:
    """Derive all Trader metrics from raw trade list + leaderboard snapshot."""
    total_volume = 0.0
    politics_bets = 0
    politics_volume = 0.0
    finance_bets = 0
    finance_volume = 0.0
    num_bets = 0
    # Per-market tracking for win rate: {market_key: {has_buy, has_win}}
    markets: dict = {}

    for t in trades:
        vol = t["volume_usd"]
        cat = t["category"]
        key = t.get("market_key") or "__unknown__"

        if key not in markets:
            markets[key] = {"has_buy": False, "has_win": False}

        if t["is_bet"]:
            total_volume += vol
            num_bets += 1
            markets[key]["has_buy"] = True
            if cat == "politics":
                politics_bets += 1
                politics_volume += vol
            elif cat == "finance":
                finance_bets += 1
                finance_volume += vol
        elif t["is_win"]:
            markets[key]["has_win"] = True

    profit = leaderboard_entry["profit_usd"]
    # Leaderboard volume is the authoritative all-time figure; fall back to the
    # windowed activity total only when the leaderboard doesn't report it.
    effective_volume = leaderboard_entry["volume_usd"] or total_volume or 1
    roi = profit / effective_volume if effective_volume else 0.0

    # Win rate: per-market (markets won / markets where we observed a BUY).
    # Only count markets where we saw the BUY side — we can't count pure-REDEEM
    # entries because those may be from bets placed outside the activity window.
    # Open/unresolved positions look like losses (no REDEEM yet), so this
    # understates true win rate slightly — that's intentional and honest.
    # Require at least 5 observable markets to avoid noise.
    markets_with_buy = sum(1 for m in markets.values() if m["has_buy"])
    markets_won = sum(1 for m in markets.values() if m["has_buy"] and m["has_win"])
    _MIN_MARKETS = 5
    win_rate = (markets_won / markets_with_buy) if markets_with_buy >= _MIN_MARKETS else None

    # Avg position: use windowed volume / windowed bet count (same scope).
    avg_position = total_volume / num_bets if num_bets else 0.0

    consistency_score = win_rate  # mirrors win_rate for now; refine later

    return {
        "total_profit_usd": profit,
        "total_volume_usd": leaderboard_entry["volume_usd"] or effective_volume,
        "num_bets": num_bets,
        "win_rate": win_rate,
        "roi": roi,
        "avg_position_usd": avg_position,
        "politics_bets": politics_bets,
        "politics_volume_usd": politics_volume,
        "finance_bets": finance_bets,
        "finance_volume_usd": finance_volume,
        "consistency_score": consistency_score,
        "username": leaderboard_entry.get("username"),
    }


async def _upsert_trader(session, address: str, metrics: dict) -> bool:
    """Insert or update a Trader row. Returns True if new."""
    stmt = (
        insert(Trader)
        .values(address=address, **metrics)
        .on_conflict_do_update(
            index_elements=["address"],
            set_={k: v for k, v in metrics.items()},
        )
    )
    result = await session.execute(stmt)
    # on_conflict_do_update: rowcount 1 = upserted
    return result.rowcount == 1


async def run_discovery() -> None:
    """Main discovery job — called by the scheduler loop."""
    logger.info("Starting trader discovery run")
    started_at = datetime.now(timezone.utc)

    async with AsyncSessionLocal() as session:
        # Create fetch log entry
        log = FetchLog(status="running")
        session.add(log)
        await session.commit()
        await session.refresh(log)
        log_id = log.id

    traders_discovered = 0
    traders_updated = 0
    error_msg = None

    try:
        # 1. Leaderboard
        leaderboard = await data_api_client.fetch_leaderboard(
            limit=settings.leaderboard_limit
        )
        if not leaderboard:
            raise RuntimeError("Leaderboard returned empty — API may be unavailable")

        # 2. Per-trader activity
        for entry in leaderboard:
            address = entry["address"]
            try:
                trades = await data_api_client.fetch_user_activity(address)
                metrics = _compute_metrics(entry, trades)

                async with AsyncSessionLocal() as session:
                    is_new = await _upsert_trader(session, address, metrics)
                    await session.commit()

                if is_new:
                    traders_discovered += 1
                else:
                    traders_updated += 1

            except Exception as e:
                logger.warning(f"Failed processing trader {address}: {e}")

            await asyncio.sleep(_INTER_REQUEST_DELAY)

        # 3. Invalidate cache so next API read reflects fresh data
        await cache.invalidate_traders_cache()

        status = "done"
        logger.info(
            f"Discovery done: {traders_discovered} new, {traders_updated} updated"
        )

    except Exception as e:
        status = "error"
        error_msg = str(e)
        logger.error(f"Discovery run failed: {e}")

    # Update fetch log
    async with AsyncSessionLocal() as session:
        await session.execute(
            update(FetchLog)
            .where(FetchLog.id == log_id)
            .values(
                finished_at=datetime.now(timezone.utc),
                traders_discovered=traders_discovered,
                traders_updated=traders_updated,
                status=status,
                error=error_msg,
            )
        )
        await session.commit()


async def scheduler_loop() -> None:
    """Background asyncio task that runs discovery every N hours."""
    interval = settings.scheduler_interval_hours * 3600
    logger.info(f"Scheduler started — interval: {settings.scheduler_interval_hours}h")

    # Run immediately on first startup, then repeat
    while True:
        try:
            await run_discovery()
        except Exception as e:
            logger.error(f"Unhandled error in scheduler_loop: {e}")
        await asyncio.sleep(interval)
