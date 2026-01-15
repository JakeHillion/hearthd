"""Config entries module for hearthd."""

import asyncio
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
                # Create add_entities callback - must be sync because HA
                # integrations call it without await
                def async_add_entities(entities, update_before_add=False):
                    """Add entities to Home Assistant."""
                    async def _register_entities():
                        for entity in entities:
                            _LOGGER.info("Adding entity: %s", getattr(entity, 'name', 'unknown'))
                            # Store hass reference on entity for state updates
                            entity.hass = hass
                            # Register entity with Rust
                            if hasattr(hass, '_send_message'):
                                # Build device_info if available
                                device_info = None
                                di = getattr(entity, '_attr_device_info', None)
                                if di is not None:
                                    device_info = {
                                        "identifiers": [list(i) for i in getattr(di, 'identifiers', [])],
                                        "name": getattr(di, 'name', ''),
                                        "manufacturer": getattr(di, 'manufacturer', None),
                                        "model": getattr(di, 'model', None),
                                        "sw_version": getattr(di, 'sw_version', None),
                                    }

                                # Resolve unique_id: prefer property, fall back to _attr
                                uid = getattr(entity, '_attr_unique_id', None)
                                try:
                                    uid = entity.unique_id or uid
                                except Exception:
                                    pass
                                uid = uid or 'unknown'

                                # Resolve name: prefer _attr_name, fall back to property
                                ename = getattr(entity, '_attr_name', None)
                                if ename is None:
                                    try:
                                        ename = entity.name
                                    except Exception:
                                        pass
                                ename = ename or 'Unknown'

                                msg = {
                                    "type": "entity_register",
                                    "entity_id": f"{platform_str}.{uid}",
                                    "name": ename,
                                    "platform": platform_str,
                                    "device_class": getattr(entity, 'device_class', None),
                                    "capabilities": {"supported_features": getattr(entity, 'supported_features', 0)},
                                }
                                if device_info is not None:
                                    msg["device_info"] = device_info
                                await hass._send_message(msg)

                            # Send initial state for entities that support it
                            send_fn = getattr(entity, 'async_send_state_to_rust', None)
                            if send_fn is not None:
                                try:
                                    await send_fn()
                                except Exception:
                                    _LOGGER.debug("Failed to send initial state", exc_info=True)
                    asyncio.ensure_future(_register_entities())

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
