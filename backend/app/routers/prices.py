from fastapi import APIRouter, Query
from app.services import clob_client

router = APIRouter()


@router.get("/prices/history")
async def get_price_history(
    token_id: str = Query(..., description="CLOB token ID (YES token)"),
    interval: str = Query("1w", description="Time interval: 1d, 1w, 1m, 3m, 1y, all"),
):
    return await clob_client.get_price_history(token_id=token_id, interval=interval)
