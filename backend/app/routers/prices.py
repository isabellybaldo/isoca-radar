from typing import Literal
from fastapi import APIRouter, Query, Request
from app.limiter import limiter
from app.services import clob_client

router = APIRouter()


@router.get("/prices/history")
@limiter.limit("30/minute")
async def get_price_history(
    request: Request,
    token_id: str = Query(..., min_length=1, description="CLOB token ID (YES token)"),
    interval: Literal["1d", "1w", "1m", "3m", "1y", "all"] = Query("1w", description="Time interval"),
):
    return await clob_client.get_price_history(token_id=token_id, interval=interval)
