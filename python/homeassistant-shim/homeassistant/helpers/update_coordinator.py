"""Update coordinator helper for hearthd."""

import asyncio
import logging
from datetime import timedelta
from typing import Any, Callable, Generic, TypeVar

from homeassistant.core import HomeAssistant, callback
from homeassistant.exceptions import HomeAssistantError

_LOGGER = logging.getLogger(__name__)

T = TypeVar("T")


class UpdateFailed(HomeAssistantError):
    """Update failed exception."""

    pass


class DataUpdateCoordinator(Generic[T]):
    """Data update coordinator base class."""

    def __init__(
        self,
        hass: HomeAssistant,
        logger: logging.Logger,
        *,
        name: str,
        update_interval: timedelta | None = None,
        update_method: Callable[[], Any] | None = None,
    ):
        self.hass = hass
        self.logger = logger
        self.name = name
        self.update_interval = update_interval
        self._update_method = update_method

        self.data: T | None = None
        self.last_update_success = True
        self._listeners: list[Callable] = []
        self._update_task: asyncio.Task | None = None

    async def async_config_entry_first_refresh(self) -> None:
        """Refresh data for the first time when a config entry is setup."""
        await self.async_refresh()

    async def async_refresh(self) -> None:
        """Refresh data."""
        try:
            if self._update_method:
                data = await self._update_method()
            else:
                data = await self._async_update_data()

            self.data = data
            self.last_update_success = True
            self.logger.debug("Finished fetching %s data", self.name)

            # Notify listeners
            await self._async_notify_listeners()

        except Exception as err:
            self.last_update_success = False
            self.logger.error("Error fetching %s data: %s", self.name, err)
            raise UpdateFailed(f"Error fetching {self.name} data") from err

    async def _async_update_data(self) -> T:
        """Fetch data. Override this method."""
        raise NotImplementedError("Update method not implemented")

    @callback
    def async_add_listener(self, update_callback: Callable) -> Callable:
        """Add a listener for updates."""
        self._listeners.append(update_callback)

        def remove_listener() -> None:
            self._listeners.remove(update_callback)

        return remove_listener

    async def _async_notify_listeners(self) -> None:
        """Notify all listeners."""
        for listener in self._listeners:
            if asyncio.iscoroutinefunction(listener):
                await listener()
            else:
                listener()

    async def async_request_refresh(self) -> None:
        """Request a refresh."""
        await self.async_refresh()


class CoordinatorEntity(Generic[T]):
    """Base class for entities that use a DataUpdateCoordinator."""

    def __init__(self, coordinator: T):
        """Initialize coordinator entity."""
        self.coordinator = coordinator
        # Register as listener
        coordinator.async_add_listener(self._handle_coordinator_update)

    @callback
    def _handle_coordinator_update(self) -> None:
        """Handle updated data from the coordinator."""
        # Subclasses can override to handle updates
        pass
