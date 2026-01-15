"""aiohttp client helper - thin wrapper around real aiohttp."""

import aiohttp
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from homeassistant.core import HomeAssistant


def async_get_clientsession(hass: "HomeAssistant") -> aiohttp.ClientSession:
    """Get aiohttp client session."""
    # Reuse existing session if available
    if "aiohttp_session" not in hass.data:
        hass.data["aiohttp_session"] = aiohttp.ClientSession()
    return hass.data["aiohttp_session"]
