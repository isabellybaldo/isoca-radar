from contextlib import asynccontextmanager
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.config import settings
from app.routers import markets, prices
from app.services import gamma_client, clob_client


@asynccontextmanager
async def lifespan(app: FastAPI):
    await gamma_client.startup()
    await clob_client.startup()
    yield
    await gamma_client.shutdown()
    await clob_client.shutdown()


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


@app.get("/health")
async def health():
    return {"status": "ok"}
