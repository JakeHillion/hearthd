"""Core Home Assistant stub for hearthd sandbox."""

import asyncio
import json
import logging
from pathlib import Path
from typing import Any, Callable
from functools import wraps

_LOGGER = logging.getLogger(__name__)


def callback(func: Callable) -> Callable:
    """Decorator to mark a function as a callback."""
    @wraps(func)
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)
    return wrapper


class UnitSystem:
    """Unit system stub."""

    def __init__(self):
        self.name = "metric"

    @property
    def temperature_unit(self):
        return "°C"


METRIC_SYSTEM = UnitSystem()


class Config:
    """Configuration object."""

    def __init__(self):
        self.latitude: float = 0.0
        self.longitude: float = 0.0
        self.elevation: int = 0
        self.time_zone: str = "UTC"
        self.components: set[str] = set()
        self.config_dir: str = "/tmp/hearthd"
        self.location_name: str = "Home"
        self.units = METRIC_SYSTEM


class ConfigEntries:
    """Config entries registry stub."""

    def __init__(self, hass: "HomeAssistant"):
        self.hass = hass
        self._entries: dict[str, "ConfigEntry"] = {}

    def async_entries(self, domain: str | None = None) -> list["ConfigEntry"]:
        """Get all config entries or filter by domain."""
        if domain is None:
            return list(self._entries.values())
        return [e for e in self._entries.values() if e.domain == domain]


class ConfigEntry:
    """Config entry stub."""

    def __init__(self, entry_id: str, domain: str, data: dict[str, Any]):
        self.entry_id = entry_id
        self.domain = domain
        self.data = data
        self.runtime_data: Any = None
        self._on_unload_callbacks: list[Callable] = []

    def async_on_unload(self, func: Callable) -> None:
        """Register a callback to call on unload."""
        self._on_unload_callbacks.append(func)


class Event:
    """Event object."""

    def __init__(self, event_type: str, data: dict[str, Any] | None = None):
        self.event_type = event_type
        self.data = data or {}


class HomeAssistant:
    """Main Home Assistant class - communicates with Rust via Unix socket."""

    def __init__(self, socket_path: str = "/tmp/hearthd.sock"):
        self.socket_path = socket_path
        self.config = Config()
        self.data: dict[str, Any] = {}
        self.states = StateRegistry(self)
        self.bus = EventBus(self)
        self.services = ServiceRegistry(self)
        self.config_entries = ConfigEntries(self)
        self.loop = asyncio.get_event_loop()

        self._reader: asyncio.StreamReader | None = None
        self._writer: asyncio.StreamWriter | None = None

    async def async_start(self):
        """Start the Home Assistant instance and connect to Rust."""
        _LOGGER.info("Connecting to hearthd at %s", self.socket_path)
        self._reader, self._writer = await asyncio.open_unix_connection(
            self.socket_path
        )

        # Send ready message
        await self._send_message({"type": "ready"})
        _LOGGER.info("Connected to hearthd")

    async def async_stop(self):
        """Stop the Home Assistant instance."""
        if self._writer:
            self._writer.close()
            await self._writer.wait_closed()

    async def _send_message(self, message: dict[str, Any]):
        """Send a message to Rust over the socket."""
        if not self._writer:
            raise RuntimeError("Not connected to hearthd")

        data = json.dumps(message).encode() + b"\n"
        self._writer.write(data)
        await self._writer.drain()

    async def _recv_message(self) -> dict[str, Any] | None:
        """Receive a message from Rust."""
        if not self._reader:
            return None

        line = await self._reader.readline()
        if not line:
            return None

        return json.loads(line.decode())


class StateRegistry:
    """State registry - sends state updates to Rust."""

    def __init__(self, hass: HomeAssistant):
        self.hass = hass
        self._states: dict[str, dict[str, Any]] = {}

    async def async_set(
        self,
        entity_id: str,
        state: str,
        attributes: dict[str, Any] | None = None,
        force_update: bool = False,
    ):
        """Set entity state and send to Rust."""
        self._states[entity_id] = {
            "state": state,
            "attributes": attributes or {},
        }

        await self.hass._send_message({
            "type": "state_update",
            "entity_id": entity_id,
            "state": state,
            "attributes": attributes or {},
        })

    def get(self, entity_id: str) -> dict[str, Any] | None:
        """Get entity state."""
        return self._states.get(entity_id)


class EventBus:
    """Event bus stub."""

    def __init__(self, hass: HomeAssistant):
        self.hass = hass
        self._listeners: dict[str, list[Callable]] = {}

    async def async_fire(self, event_type: str, event_data: dict[str, Any] | None = None):
        """Fire an event."""
        _LOGGER.debug("Event: %s - %s", event_type, event_data)

        # Call registered listeners
        if event_type in self._listeners:
            event = Event(event_type, event_data)
            for listener in self._listeners[event_type]:
                if asyncio.iscoroutinefunction(listener):
                    await listener(event)
                else:
                    listener(event)

    def async_listen(self, event_type: str, listener: Callable) -> Callable:
        """Listen for events of a specific type."""
        if event_type not in self._listeners:
            self._listeners[event_type] = []
        self._listeners[event_type].append(listener)

        # Return a function that can be called to remove the listener
        def remove_listener():
            if event_type in self._listeners:
                self._listeners[event_type].remove(listener)

        return remove_listener


class ServiceRegistry:
    """Service registry stub."""

    def __init__(self, hass: HomeAssistant):
        self.hass = hass
        self._services: dict[str, dict[str, Any]] = {}

    async def async_register(
        self,
        domain: str,
        service: str,
        service_func,
        schema=None,
    ):
        """Register a service."""
        if domain not in self._services:
            self._services[domain] = {}
        self._services[domain][service] = service_func
        _LOGGER.debug("Registered service: %s.%s", domain, service)
