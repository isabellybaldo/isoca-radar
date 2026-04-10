from fastapi import APIRouter, HTTPException, Query, Request
from app.limiter import limiter
from app.services import gamma_client, clob_client

router = APIRouter()


@router.get("/markets")
@limiter.limit("60/minute")
async def list_markets(
    request: Request,
    limit: int = Query(20, ge=1, le=100),
    offset: int = Query(0, ge=0),
):
    return await gamma_client.list_markets(limit=limit, offset=offset)


@router.get("/markets/{slug}")
@limiter.limit("60/minute")
async def get_market(request: Request, slug: str):
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
