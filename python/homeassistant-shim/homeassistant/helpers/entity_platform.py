"""Entity platform helper for hearthd."""

from collections.abc import Callable
from typing import Any


# Type alias for the callback used in async_setup_entry
AddConfigEntryEntitiesCallback = Callable[[list[Any]], None]


class AddEntitiesCallback:
    """Callback for adding entities."""

    def __init__(self):
        self.entities: list[Any] = []

    def __call__(self, entities: list[Any]) -> None:
        """Add entities."""
        self.entities.extend(entities)
