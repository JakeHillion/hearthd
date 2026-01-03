"""Update coordinator helper for hearthd."""

import asyncio
import logging
import uuid
from datetime import timedelta
from typing import Any, Callable, Generic, TypeVar

from homeassistant.core import HomeAssistant, callback
from homeassistant.exceptions import HomeAssistantError

_LOGGER = logging.getLogger(__name__)

T = TypeVar("T")


class UpdateFailed(HomeAssistantError):
    """Update failed exception."""

    def __init__(
        self,
        message: str = "",
        *,
        translation_domain: str | None = None,
        translation_key: str | None = None,
        translation_placeholders: dict | None = None,
    ):
        """Initialize update failed exception."""
        super().__init__(message)
        self.translation_domain = translation_domain
        self.translation_key = translation_key
        self.translation_placeholders = translation_placeholders or {}


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
        config_entry: Any = None,  # Optional ConfigEntry reference
    ):
        self.hass = hass
        self.logger = logger
        self.name = name
        self.update_interval = update_interval
        self._update_method = update_method
        self.config_entry = config_entry  # Store config entry reference

        self.data: T | None = None
        self.last_update_success = True
        self._listeners: list[Callable] = []
        self._update_task: asyncio.Task | None = None

        # Generate a unique timer ID for this coordinator
        self._timer_id = f"{name}_{uuid.uuid4().hex[:8]}"

    async def async_config_entry_first_refresh(self) -> None:
        """Refresh data for the first time when a config entry is setup."""
        # First do the initial refresh
        await self.async_refresh()

        # Then register timer with Rust for periodic updates
        if self.update_interval and hasattr(self.hass, '_send_message'):
            interval_seconds = int(self.update_interval.total_seconds())
            _LOGGER.info(
                "Registering timer %s for %s with interval %ds",
                self._timer_id, self.name, interval_seconds
            )

            # Register this coordinator in hass for TriggerUpdate lookup
            self.hass._coordinators[self._timer_id] = self

            # Send ScheduleUpdate message to Rust
            await self.hass._send_message({
                "type": "schedule_update",
                "timer_id": self._timer_id,
                "name": self.name,
                "interval_seconds": interval_seconds,
            })

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
