from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    cors_origins: str = "http://localhost:5173"
    gamma_api_url: str = "https://gamma-api.polymarket.com"
    clob_api_url: str = "https://clob.polymarket.com"
    data_api_url: str = "https://data-api.polymarket.com"

    database_url: str = "postgresql+asyncpg://postgres:postgres@localhost:5432/cute_trading"
    redis_url: str = "redis://localhost:6379/0"

    postgres_user: str = "postgres"
    postgres_password: str = "postgres"
    postgres_db: str = "cute_trading"
    postgres_schema: str = "cute_trading"

    polymarket_api_key: str = ""
    scheduler_interval_hours: int = 6
    leaderboard_limit: int = 100

    model_config = {"env_file": ".env"}

    @property
    def cors_origins_list(self) -> list[str]:
        return [o.strip() for o in self.cors_origins.split(",")]


settings = Settings()
