"""
Client for data-api.polymarket.com

Key endpoints used:
  GET /rankings?window=all&limit=N   — leaderboard (all-time profit)
  GET /activity?user={addr}&limit=N  — trade history for a wallet

NOTE: These endpoints are inferred from Polymarket's public API surface.
      If responses differ, adjust _parse_ranking() / _parse_trade() accordingly.
"""

import asyncio
import logging
from typing import Any

import httpx

from app.config import settings

logger = logging.getLogger(__name__)

_client: httpx.AsyncClient | None = None

# Categories as Polymarket returns them (case-insensitive matched below)
POLITICS_TAGS = {"politics", "political", "elections", "government", "election"}
FINANCE_TAGS = {"crypto", "finance", "economics", "financial markets", "business", "markets"}


async def startup():
    global _client
    headers = {"Content-Type": "application/json"}
    if settings.polymarket_api_key:
        headers["Authorization"] = f"Bearer {settings.polymarket_api_key}"
    _client = httpx.AsyncClient(
        base_url=settings.data_api_url,
        timeout=30.0,
        headers=headers,
    )


async def shutdown():
    if _client:
        await _client.aclose()


async def _get(path: str, params: dict | None = None, retries: int = 3) -> Any:
    """GET with simple exponential backoff on 429 / 5xx."""
    for attempt in range(retries):
        try:
            resp = await _client.get(path, params=params)
            if resp.status_code == 429:
                wait = 2 ** attempt * 5
                logger.warning(f"Rate limited on {path}, waiting {wait}s")
                await asyncio.sleep(wait)
                continue
            resp.raise_for_status()
            return resp.json()
        except httpx.HTTPStatusError as e:
            if attempt == retries - 1:
                logger.error(f"HTTP error on {path}: {e}")
                return None
            await asyncio.sleep(2 ** attempt)
        except Exception as e:
            if attempt == retries - 1:
                logger.error(f"Request failed on {path}: {e}")
                return None
            await asyncio.sleep(2 ** attempt)
    return None


def _normalize_category(raw: str | None) -> str | None:
    """Map raw category string to 'politics', 'finance', or None."""
    if not raw:
        return None
    lower = raw.lower().strip()
    if any(tag in lower for tag in POLITICS_TAGS):
        return "politics"
    if any(tag in lower for tag in FINANCE_TAGS):
        return "finance"
    return None


def _parse_ranking(item: dict) -> dict | None:
    """Normalise a single leaderboard entry."""
    address = item.get("proxyWallet") or item.get("address") or item.get("wallet")
    if not address:
        return None
    return {
        "address": address.lower(),
        "username": item.get("userName") or item.get("name") or item.get("username"),
        "profit_usd": float(item.get("pnl") or item.get("profit") or 0),
        "volume_usd": float(item.get("vol") or item.get("volume") or 0),
    }


def _parse_trade(item: dict) -> dict | None:
    """Normalise a single activity entry.

    The /activity endpoint has no resolved-outcome field.
    We infer wins from REDEEM transactions: a REDEEM means the trader
    held the winning side of a resolved market.
    """
    trade_type = item.get("type", "").upper()
    side = item.get("side", "").upper()

    # Only care about BUY trades and REDEEMs
    is_bet = trade_type == "TRADE" and side == "BUY"
    is_win = trade_type == "REDEEM"
    if not is_bet and not is_win:
        return None

    # Volume: usdcSize is the USD notional; fallback to size * price
    usd_size = float(item.get("usdcSize") or 0)
    if not usd_size:
        size = float(item.get("size") or 0)
        price = float(item.get("price") or 0)
        usd_size = size * price if price else size

    # Category from market title heuristics (activity has no explicit category field)
    title = (item.get("title") or item.get("name") or "").lower()
    category = None
    if any(tag in title for tag in POLITICS_TAGS):
        category = "politics"
    elif any(tag in title for tag in FINANCE_TAGS):
        category = "finance"

    # Market identifier for per-market win-rate tracking.
    # conditionId is the stable Polymarket condition identifier; title is the fallback.
    market_key = item.get("conditionId") or title or None

    return {
        "market_key": market_key,
        "category": category,
        "volume_usd": usd_size,
        "is_bet": is_bet,
        "is_win": is_win,
    }


async def fetch_leaderboard(limit: int = 100) -> list[dict]:
    """Return list of normalised trader dicts from the all-time leaderboard.

    The /v1/leaderboard endpoint caps at 50 per page, so we paginate.
    """
    results = []
    page_size = 50  # API max
    offset = 0

    while len(results) < limit:
        batch_limit = min(page_size, limit - len(results))
        data = await _get(
            "/v1/leaderboard",
            params={
                "timePeriod": "ALL",
                "orderBy": "PNL",
                "limit": batch_limit,
                "offset": offset,
            },
        )
        if not data:
            break
        if isinstance(data, dict):
            data = data.get("data") or data.get("rankings") or []
        if not data:
            break
        for item in data:
            parsed = _parse_ranking(item)
            if parsed:
                results.append(parsed)
        if len(data) < batch_limit:
            break  # last page
        offset += batch_limit

    logger.info(f"Fetched {len(results)} traders from leaderboard")
    return results


async def fetch_user_activity(address: str, limit: int = 500) -> list[dict]:
    """Return normalised trade list for a wallet address."""
    data = await _get("/activity", params={"user": address, "limit": limit})
    if not data:
        return []
    if isinstance(data, dict):
        data = data.get("data") or data.get("activity") or []
    results = []
    for item in data:
        parsed = _parse_trade(item)
        if parsed:
            results.append(parsed)
    return results
