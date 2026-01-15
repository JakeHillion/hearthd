"""Device registry stub for hearthd."""

from enum import StrEnum
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from homeassistant.core import HomeAssistant


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


class DeviceRegistry:
    """Device registry stub."""

    def __init__(self, hass: "HomeAssistant"):
        self.hass = hass
        self._devices: dict[str, DeviceEntry] = {}

    def async_get_device(self, identifiers: set | None = None) -> DeviceEntry | None:
        """Get device by identifiers."""
        return None

    def async_remove_device(self, device_id: str) -> None:
        """Remove a device."""
        self._devices.pop(device_id, None)


def async_get(hass: "HomeAssistant") -> DeviceRegistry:
    """Get device registry."""
    if "device_registry" not in hass.data:
        hass.data["device_registry"] = DeviceRegistry(hass)
    return hass.data["device_registry"]
