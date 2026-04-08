import httpx
from app.config import settings

_client: httpx.AsyncClient | None = None


def get_client() -> httpx.AsyncClient:
    return _client


async def startup():
    global _client
    _client = httpx.AsyncClient(base_url=settings.clob_api_url, timeout=10.0)


async def shutdown():
    global _client
    if _client:
        await _client.aclose()


async def get_prices(token_ids: list[str]) -> dict:
    if not token_ids:
        return {}
    resp = await _client.get("/prices", params={"token_ids": ",".join(token_ids)})
    resp.raise_for_status()
    return resp.json()


async def get_price_history(token_id: str, interval: str = "1w") -> list[dict]:
    resp = await _client.get(
        "/prices-history",
        params={"market": token_id, "interval": interval, "fidelity": 60},
    )
    resp.raise_for_status()
    data = resp.json()
    history = data.get("history", data) if isinstance(data, dict) else data
    return [{"t": p.get("t"), "p": p.get("p")} for p in history]
