from datetime import datetime, timezone

from sqlalchemy import Column, String, Float, Integer, DateTime, Text
from sqlalchemy.sql import func

from app.db import Base


class Trader(Base):
    __tablename__ = "traders"

    address = Column(String, primary_key=True)
    username = Column(String, nullable=True)

    # Overall metrics
    total_profit_usd = Column(Float, default=0.0)
    total_volume_usd = Column(Float, default=0.0)
    num_bets = Column(Integer, default=0)
    win_rate = Column(Float, default=0.0)       # 0-1
    roi = Column(Float, default=0.0)             # profit/volume, can be negative
    avg_position_usd = Column(Float, default=0.0)

    # Category-specific
    politics_bets = Column(Integer, default=0)
    politics_volume_usd = Column(Float, default=0.0)
    finance_bets = Column(Integer, default=0)
    finance_volume_usd = Column(Float, default=0.0)

    # Consistency: fraction of weekly windows where trader was profitable
    consistency_score = Column(Float, default=0.0)  # 0-1

    first_seen = Column(DateTime(timezone=True), server_default=func.now())
    last_updated = Column(DateTime(timezone=True), server_default=func.now(), onupdate=func.now())


class FetchLog(Base):
    __tablename__ = "fetch_logs"

    id = Column(Integer, primary_key=True, autoincrement=True)
    started_at = Column(DateTime(timezone=True), server_default=func.now())
    finished_at = Column(DateTime(timezone=True), nullable=True)
    traders_discovered = Column(Integer, default=0)
    traders_updated = Column(Integer, default=0)
    status = Column(String, default="running")   # running | done | error
    error = Column(Text, nullable=True)
