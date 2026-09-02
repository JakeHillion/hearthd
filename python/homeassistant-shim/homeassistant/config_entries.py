"""Config entries module for hearthd."""

import contextlib
import importlib
import logging
from typing import TYPE_CHECKING
from typing import Any

from homeassistant.core import ConfigEntry

if TYPE_CHECKING:
    from homeassistant.core import HomeAssistant

_LOGGER = logging.getLogger(__name__)


def _device_info(entity: Any) -> dict[str, Any] | None:
    """Describe the device an entity belongs to, if it declares one."""
    info = getattr(entity, "_attr_device_info", None)
    if info is None:
        return None

    return {
        "identifiers": [list(i) for i in getattr(info, "identifiers", [])],
        "name": getattr(info, "name", ""),
        "manufacturer": getattr(info, "manufacturer", None),
        "model": getattr(info, "model", None),
        "sw_version": getattr(info, "sw_version", None),
    }


def _unique_id(entity: Any) -> str:
    """Resolve an entity's unique id, preferring the property to the attribute.

    Integrations set either, and the property can raise when it depends on data
    the integration has not fetched yet, so the attribute is the fallback.
    """
    uid = getattr(entity, "_attr_unique_id", None)
    with contextlib.suppress(Exception):
        uid = entity.unique_id or uid
    return uid or "unknown"


def _name(entity: Any) -> str:
    """Resolve an entity's display name, preferring the explicit attribute."""
    name = getattr(entity, "_attr_name", None)
    if name is None:
        with contextlib.suppress(Exception):
            name = entity.name
    return name or "Unknown"


async def _register_entity(hass: HomeAssistant, entity: Any, platform: str) -> None:
    """Announce one entity to Rust and send whatever state it already has."""
    _LOGGER.info("Adding entity: %s", getattr(entity, "name", "unknown"))

    # Entities reach back through hass to send their own state updates.
    entity.hass = hass

    if not hasattr(hass, "_send_message"):
        return

    message = {
        "type": "entity_register",
        "entity_id": f"{platform}.{_unique_id(entity)}",
        "name": _name(entity),
        "platform": platform,
        "device_class": getattr(entity, "device_class", None),
        "capabilities": {
            "supported_features": getattr(entity, "supported_features", 0)
        },
    }
    device_info = _device_info(entity)
    if device_info is not None:
        message["device_info"] = device_info

    await hass._send_message(message)

    send_state = getattr(entity, "async_send_state_to_rust", None)
    if send_state is not None:
        try:
            await send_state()
        except Exception:
            _LOGGER.debug("Failed to send initial state", exc_info=True)


async def _setup_platform(
    hass: HomeAssistant,
    entry: ConfigEntry,
    domain: str,
    platform: str,
) -> None:
    """Set one platform up and register everything it produces.

    Registration is awaited here rather than handed to a background task. Rust
    accepts ``entity_register`` only once the integration has finished setting
    up, and a task that is merely scheduled is not ordered against the
    ``setup_complete`` that follows it. Losing that race drops the entity
    silently, and with it every state update that ever refers to it, for the
    life of the process.
    """
    module_name = f"homeassistant.components.{domain}.{platform}"
    _LOGGER.info("Setting up platform: %s", module_name)

    module = importlib.import_module(module_name)
    if not hasattr(module, "async_setup_entry"):
        return

    # Home Assistant calls this synchronously, so it can only collect; the
    # awaiting happens below, once the platform has finished adding entities.
    pending: list[Any] = []

    def async_add_entities(entities, update_before_add=False):
        """Add entities to Home Assistant."""
        pending.extend(entities)

    await module.async_setup_entry(hass, entry, async_add_entities)

    for entity in pending:
        await _register_entity(hass, entity, platform)

    _LOGGER.info("Platform %s setup complete", platform)


async def async_forward_entry_setups(
    hass: HomeAssistant,
    entry: ConfigEntry,
    platforms: list[str],
) -> bool:
    """Forward setup of platforms for a config entry."""
    _LOGGER.info("Forwarding entry setups for platforms: %s", platforms)

    # Recorded so that setup_complete can report which platforms came up.
    if not hasattr(entry, "_forwarded_platforms"):
        entry._forwarded_platforms = []

    for platform in platforms:
        # Platform arrives either as a plain string or as Home Assistant's
        # Platform enum, whose value is the string we want.
        name = getattr(platform, "value", None) or str(platform)
        entry._forwarded_platforms.append(name)

        try:
            await _setup_platform(hass, entry, entry.domain, name)
        except ImportError as e:
            _LOGGER.warning("Could not import platform %s: %s", name, e)
        except Exception as e:
            _LOGGER.error("Error setting up platform %s: %s", name, e, exc_info=True)

    return True


async def async_unload_platforms(
    hass: HomeAssistant,
    entry: ConfigEntry,
    platforms: list[str],
) -> bool:
    """Unload platforms for a config entry."""
    _LOGGER.info("Unloading platforms: %s", platforms)
    # TODO: Implement proper unload
    return True


__all__ = ["ConfigEntry", "async_forward_entry_setups", "async_unload_platforms"]
