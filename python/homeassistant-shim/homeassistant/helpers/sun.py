"""Sun helper for hearthd."""

from homeassistant.core import HomeAssistant


def is_up(hass: HomeAssistant) -> bool:
    """Return True if sun is up."""
    # Simple stub - assume daytime
    return True
