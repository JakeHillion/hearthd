"""Constants for Home Assistant."""

from enum import StrEnum

__version__ = "2024.12.0"

# Platforms
class Platform(StrEnum):
    """Available entity platforms."""

    BINARY_SENSOR = "binary_sensor"
    SENSOR = "sensor"
    SWITCH = "switch"
    WEATHER = "weather"
    # Add more as needed

# Config keys
CONF_API_KEY = "api_key"
CONF_LATITUDE = "latitude"
CONF_LONGITUDE = "longitude"
CONF_NAME = "name"

# Entity attributes
ATTR_ATTRIBUTION = "attribution"
ATTR_DEVICE_CLASS = "device_class"
ATTR_ENTITY_PICTURE = "entity_picture"
ATTR_FRIENDLY_NAME = "friendly_name"
ATTR_ICON = "icon"
ATTR_SUPPORTED_FEATURES = "supported_features"
ATTR_UNIT_OF_MEASUREMENT = "unit_of_measurement"

# States
STATE_ON = "on"
STATE_OFF = "off"
STATE_UNAVAILABLE = "unavailable"
STATE_UNKNOWN = "unknown"
