import json
import time
import httpx
from app.config import settings

_client: httpx.AsyncClient | None = None
_cache: dict = {}
CACHE_TTL = 60


def get_client() -> httpx.AsyncClient:
    return _client


async def startup():
    global _client
    _client = httpx.AsyncClient(base_url=settings.gamma_api_url, timeout=10.0)


async def shutdown():
    global _client
    if _client:
        await _client.aclose()


def _cached(key: str, data):
    _cache[key] = {"data": data, "ts": time.time()}


def _get_cache(key: str):
    entry = _cache.get(key)
    if entry and time.time() - entry["ts"] < CACHE_TTL:
        return entry["data"]
    return None


def _parse_market(m: dict) -> dict:
    try:
        outcome_prices = json.loads(m.get("outcomePrices", "[]"))
    except (json.JSONDecodeError, TypeError):
        outcome_prices = []

    try:
        clob_token_ids = json.loads(m.get("clobTokenIds", "[]"))
    except (json.JSONDecodeError, TypeError):
        clob_token_ids = m.get("clobTokenIds") or []

    return {
        "id": m.get("id"),
        "slug": m.get("slug"),
        "question": m.get("question"),
        "description": m.get("description"),
        "outcomes": json.loads(m.get("outcomes", '["Yes","No"]')) if isinstance(m.get("outcomes"), str) else m.get("outcomes", ["Yes", "No"]),
        "outcomePrices": outcome_prices,
        "clobTokenIds": clob_token_ids,
        "volume": m.get("volume"),
        "liquidity": m.get("liquidity"),
        "startDate": m.get("startDate"),
        "endDate": m.get("endDate"),
        "closed": m.get("closed"),
        "image": m.get("image"),
        "icon": m.get("icon"),
        "conditionId": m.get("conditionId"),
        "tags": [t.get("slug") for t in m.get("tags", [])],
    }


TARGET_TAGS = ["politics", "finance"]


async def _fetch_markets_for_tag(tag: str, limit: int) -> list[dict]:
    resp = await _client.get(
        "/events",
        params={"limit": limit, "tag_slug": tag, "closed": "false", "active": "true"},
    )
    resp.raise_for_status()
    markets = []
    for event in resp.json():
        event_tags = event.get("tags", [])
        for m in event.get("markets", []):
            if not m.get("closed") and m.get("active", True):
                m_with_tags = {**m, "tags": event_tags}
                markets.append(_parse_market(m_with_tags))
    return markets


async def list_markets(limit: int = 20, offset: int = 0) -> list[dict]:
    cache_key = f"markets:tagged:{limit}:{offset}"
    cached = _get_cache(cache_key)
    if cached:
        return cached

    seen_ids: set = set()
    all_markets: list[dict] = []
    for tag in TARGET_TAGS:
        for m in await _fetch_markets_for_tag(tag, limit):
            if m["id"] not in seen_ids:
                seen_ids.add(m["id"])
                all_markets.append(m)

    page = all_markets[offset: offset + limit]
    _cached(cache_key, page)
    return page


async def get_market(slug: str) -> dict | None:
    resp = await _client.get(f"/markets/{slug}")
    if resp.status_code == 404:
        return None
    resp.raise_for_status()
    return _parse_market(resp.json())
