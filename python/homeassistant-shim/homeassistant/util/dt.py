"""Datetime utilities for hearthd."""

from datetime import UTC
from datetime import datetime
from zoneinfo import ZoneInfo

# Default time zone (can be updated)
_DEFAULT_TIME_ZONE: ZoneInfo = ZoneInfo("UTC")


def utcnow() -> datetime:
    """Get now in UTC time."""
    return datetime.now(UTC)


def as_utc(dattim: datetime) -> datetime:
    """Return a datetime as UTC."""
    if dattim.tzinfo is None:
        return dattim.replace(tzinfo=UTC)
    return dattim.astimezone(UTC)


def get_default_time_zone() -> ZoneInfo:
    """Get the default time zone."""
    return _DEFAULT_TIME_ZONE


def set_default_time_zone(time_zone: ZoneInfo) -> None:
    """Set the default time zone."""
    global _DEFAULT_TIME_ZONE
    _DEFAULT_TIME_ZONE = time_zone
