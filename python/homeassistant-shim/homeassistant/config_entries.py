"""Config entries module for hearthd."""

import importlib
import logging
from typing import TYPE_CHECKING, Any

from homeassistant.core import ConfigEntry

if TYPE_CHECKING:
    from homeassistant.core import HomeAssistant

_LOGGER = logging.getLogger(__name__)


async def async_forward_entry_setups(
    hass: "HomeAssistant",
    entry: ConfigEntry,
    platforms: list[str],
) -> bool:
    """Forward setup of platforms for a config entry."""
    _LOGGER.info("Forwarding entry setups for platforms: %s", platforms)

    # Store platforms for later reporting
    if not hasattr(entry, '_forwarded_platforms'):
        entry._forwarded_platforms = []
    entry._forwarded_platforms.extend([str(p) for p in platforms])

    # For each platform, import and call async_setup_entry
    for platform in platforms:
        platform_str = str(platform)  # Handle Platform enum
        # Convert Platform.WEATHER to "weather"
        if hasattr(platform, 'value'):
            platform_str = platform.value

        platform_module = f"homeassistant.components.{entry.domain}.{platform_str}"
        _LOGGER.info("Setting up platform: %s", platform_module)

        try:
            mod = importlib.import_module(platform_module)
            if hasattr(mod, 'async_setup_entry'):
                # Create add_entities callback
                async def async_add_entities(entities, update_before_add=False):
                    """Add entities to Home Assistant."""
                    for entity in entities:
                        _LOGGER.info("Adding entity: %s", getattr(entity, 'name', 'unknown'))
                        # Register entity with Rust
                        if hasattr(hass, '_send_message'):
                            await hass._send_message({
                                "type": "entity_register",
                                "entity_id": f"{platform_str}.{getattr(entity, 'unique_id', 'unknown')}",
                                "name": getattr(entity, 'name', 'Unknown') or 'Unknown',
                                "platform": platform_str,
                                "device_class": getattr(entity, 'device_class', None),
                            })

                # Call platform's async_setup_entry
                await mod.async_setup_entry(hass, entry, async_add_entities)
                _LOGGER.info("Platform %s setup complete", platform_str)
        except ImportError as e:
            _LOGGER.warning("Could not import platform %s: %s", platform_module, e)
        except Exception as e:
            _LOGGER.error("Error setting up platform %s: %s", platform_module, e, exc_info=True)

    return True


async def async_unload_platforms(
    hass: "HomeAssistant",
    entry: ConfigEntry,
    platforms: list[str],
) -> bool:
    """Unload platforms for a config entry."""
    _LOGGER.info("Unloading platforms: %s", platforms)
    # TODO: Implement proper unload
    return True


__all__ = ["ConfigEntry", "async_forward_entry_setups", "async_unload_platforms"]
