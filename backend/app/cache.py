import json
import logging
from redis.asyncio import Redis

from app.config import settings

logger = logging.getLogger(__name__)

_redis: Redis | None = None

TRADERS_KEY = "traders:list"
TRADERS_TTL = 3600  # 1 hour


async def startup():
    global _redis
    _redis = Redis.from_url(settings.redis_url, decode_responses=True)


async def shutdown():
    if _redis:
        await _redis.aclose()


async def get_traders_cache() -> list[dict] | None:
    try:
        data = await _redis.get(TRADERS_KEY)
        if data:
            return json.loads(data)
    except Exception as e:
        logger.warning(f"Redis get failed: {e}")
    return None


async def set_traders_cache(traders: list[dict]) -> None:
    try:
        await _redis.set(TRADERS_KEY, json.dumps(traders), ex=TRADERS_TTL)
    except Exception as e:
        logger.warning(f"Redis set failed: {e}")


async def invalidate_traders_cache() -> None:
    try:
        await _redis.delete(TRADERS_KEY)
    except Exception as e:
        logger.warning(f"Redis delete failed: {e}")
