"""Datetime utilities for hearthd."""

from datetime import datetime, timezone


def utcnow() -> datetime:
    """Get now in UTC time."""
    return datetime.now(timezone.utc)


def as_utc(dattim: datetime) -> datetime:
    """Return a datetime as UTC."""
    if dattim.tzinfo is None:
        return dattim.replace(tzinfo=timezone.utc)
    return dattim.astimezone(timezone.utc)
