"""Device registry stub for hearthd."""

from enum import StrEnum
from typing import Any


class DeviceEntryType(StrEnum):
    """Device entry type."""

    SERVICE = "service"


class DeviceEntry:
    """Device entry stub."""

    def __init__(self, device_id: str, name: str):
        self.id = device_id
        self.name = name


class DeviceInfo:
    """Device info stub (TypedDict equivalent)."""

    def __init__(self, **kwargs: Any):
        self.identifiers = kwargs.get("identifiers", set())
        self.name = kwargs.get("name", "")
        self.manufacturer = kwargs.get("manufacturer", "")
        self.model = kwargs.get("model", "")
        self.sw_version = kwargs.get("sw_version", "")
        self.configuration_url = kwargs.get("configuration_url", "")
