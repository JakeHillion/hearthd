"""Constants for Home Assistant shim."""

from enum import StrEnum


class Platform(StrEnum):
    """Available entity platforms."""

    WEATHER = "weather"
    SENSOR = "sensor"
    BINARY_SENSOR = "binary_sensor"
    SWITCH = "switch"
    LIGHT = "light"
    CLIMATE = "climate"


# Configuration keys
CONF_NAME = "name"
CONF_LATITUDE = "latitude"
CONF_LONGITUDE = "longitude"
CONF_ELEVATION = "elevation"

# Events
EVENT_CORE_CONFIG_UPDATE = "core_config_update"


# Units - Temperature
class UnitOfTemperature(StrEnum):
    """Temperature units."""

    CELSIUS = "°C"
    FAHRENHEIT = "°F"
    KELVIN = "K"


# Units - Pressure
class UnitOfPressure(StrEnum):
    """Pressure units."""

    HPA = "hPa"
    MBAR = "mbar"
    INHG = "inHg"
    PSI = "psi"


# Units - Speed
class UnitOfSpeed(StrEnum):
    """Speed units."""

    KILOMETERS_PER_HOUR = "km/h"
    METERS_PER_SECOND = "m/s"
    MILES_PER_HOUR = "mph"
    KNOTS = "kn"


# Units - Precipitation
class UnitOfPrecipitationDepth(StrEnum):
    """Precipitation depth units."""

    MILLIMETERS = "mm"
    CENTIMETERS = "cm"
    INCHES = "in"
