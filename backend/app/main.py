import asyncio
import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app import cache
from app import db as database
from app.config import settings
from app.routers import markets, prices, traders
from app.services import gamma_client, clob_client, data_api_client
from app.services.trader_discovery import scheduler_loop

logger = logging.getLogger(__name__)

_scheduler_task: asyncio.Task | None = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    global _scheduler_task

    # Existing clients
    await gamma_client.startup()
    await clob_client.startup()

    # New services
    await data_api_client.startup()
    await database.startup()
    await cache.startup()

    # Start background discovery loop
    _scheduler_task = asyncio.create_task(scheduler_loop())
    logger.info("Background scheduler task created")

    yield

    # Shutdown
    if _scheduler_task:
        _scheduler_task.cancel()
        try:
            await _scheduler_task
        except asyncio.CancelledError:
            pass

    await gamma_client.shutdown()
    await clob_client.shutdown()
    await data_api_client.shutdown()
    await database.shutdown()
    await cache.shutdown()


app = FastAPI(title="Cute Trading API", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=settings.cors_origins_list,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(markets.router, prefix="/api")
app.include_router(prices.router, prefix="/api")
app.include_router(traders.router, prefix="/api")


@app.get("/health")
async def health():
    return {"status": "ok"}
