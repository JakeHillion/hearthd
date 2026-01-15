"""Entity registry stub for hearthd."""

from typing import Any

from homeassistant.core import HomeAssistant


class RegistryEntry:
    """Entity registry entry stub."""

    def __init__(self, entity_id: str, unique_id: str):
        self.entity_id = entity_id
        self.unique_id = unique_id
        self.disabled = False


class EntityRegistry:
    """Entity registry stub."""

    def __init__(self):
        self._entries: dict[str, RegistryEntry] = {}

    def async_get(self, entity_id: str) -> RegistryEntry | None:
        """Get entity entry."""
        return self._entries.get(entity_id)

    def async_is_registered(self, entity_id: str) -> bool:
        """Check if entity is registered."""
        return entity_id in self._entries

    def async_get_entity_id(self, domain: str, platform: str, unique_id: str) -> str | None:
        """Get entity ID from unique ID."""
        # Stub implementation
        return None

    def async_remove(self, entity_id: str) -> None:
        """Remove an entity."""
        if entity_id in self._entries:
            del self._entries[entity_id]


def async_get(hass: HomeAssistant) -> EntityRegistry:
    """Get entity registry."""
    if "entity_registry" not in hass.data:
        hass.data["entity_registry"] = EntityRegistry()
    return hass.data["entity_registry"]
