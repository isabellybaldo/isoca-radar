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

logging.basicConfig(
    level=logging.INFO,
    format="%(levelname)s: %(name)s — %(message)s",
)
logger = logging.getLogger(__name__)

_scheduler_task: asyncio.Task | None = None


async def _optional_services_startup() -> None:
    """Connect to PostgreSQL and Redis in the background.

    Runs as a fire-and-forget task so a missing DB/Redis never blocks the
    lifespan from completing and the markets page always works immediately.
    """
    global _scheduler_task
    try:
        await database.startup()
    except Exception as e:
        logger.warning(f"Database unavailable — traders features disabled: {e}")
        return
    try:
        await cache.startup()
    except Exception as e:
        logger.warning(f"Redis unavailable — caching disabled: {e}")

    _scheduler_task = asyncio.create_task(scheduler_loop())
    logger.info("Background scheduler started")


@asynccontextmanager
async def lifespan(app: FastAPI):
    # Fast HTTP clients — these never block
    await gamma_client.startup()
    await clob_client.startup()
    await data_api_client.startup()

    # DB + Redis in background so a cold/missing Postgres never stalls startup
    asyncio.create_task(_optional_services_startup())

    yield

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
