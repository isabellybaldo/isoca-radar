from sqlalchemy import MetaData, text
from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker, AsyncSession
from sqlalchemy.orm import DeclarativeBase

from app.config import settings

engine = create_async_engine(settings.database_url, echo=False, pool_pre_ping=True)
AsyncSessionLocal = async_sessionmaker(engine, expire_on_commit=False)


class Base(DeclarativeBase):
    metadata = MetaData(schema=settings.postgres_schema)


async def startup():
    """Create schema (if needed) and all tables on startup."""
    # Import models so Base picks them up before create_all
    from app.models import trader  # noqa: F401
    async with engine.begin() as conn:
        await conn.execute(text(f'CREATE SCHEMA IF NOT EXISTS "{settings.postgres_schema}"'))
        await conn.run_sync(Base.metadata.create_all)


async def shutdown():
    await engine.dispose()


async def get_db() -> AsyncSession:
    async with AsyncSessionLocal() as session:
        yield session
