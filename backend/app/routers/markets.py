from fastapi import APIRouter, HTTPException
from app.services import gamma_client, clob_client

router = APIRouter()


@router.get("/markets")
async def list_markets(limit: int = 20, offset: int = 0):
    return await gamma_client.list_markets(limit=limit, offset=offset)


@router.get("/markets/{slug}")
async def get_market(slug: str):
    market = await gamma_client.get_market(slug)
    if not market:
        raise HTTPException(status_code=404, detail="Market not found")

    token_ids = market.get("clobTokenIds", [])
    prices = {}
    if token_ids:
        try:
            prices = await clob_client.get_prices(token_ids)
        except Exception:
            pass

    return {**market, "livePrice": prices}
