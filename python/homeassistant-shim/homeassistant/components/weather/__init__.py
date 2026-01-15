"""Weather component shim for hearthd."""

from enum import IntFlag
from typing import Any, Generic, Required, TypedDict, TypeVar

from homeassistant.helpers.update_coordinator import (
    CoordinatorEntity,
    DataUpdateCoordinator,
)

# Constants from weather component
DOMAIN = "weather"

# Forecast attributes
ATTR_FORECAST_CONDITION = "condition"
ATTR_FORECAST_TIME = "datetime"
ATTR_FORECAST_CLOUD_COVERAGE = "cloud_coverage"
ATTR_FORECAST_HUMIDITY = "humidity"
ATTR_FORECAST_NATIVE_PRECIPITATION = "native_precipitation"
ATTR_FORECAST_NATIVE_TEMP = "native_temperature"
ATTR_FORECAST_NATIVE_TEMP_LOW = "native_templow"
ATTR_FORECAST_NATIVE_WIND_GUST_SPEED = "native_wind_gust_speed"
ATTR_FORECAST_NATIVE_WIND_SPEED = "native_wind_speed"
ATTR_FORECAST_PRECIPITATION_PROBABILITY = "precipitation_probability"
ATTR_FORECAST_UV_INDEX = "uv_index"
ATTR_FORECAST_WIND_BEARING = "wind_bearing"

# Weather attributes
ATTR_WEATHER_VISIBILITY = "visibility"
ATTR_WEATHER_CLOUD_COVERAGE = "cloud_coverage"
ATTR_WEATHER_DEW_POINT = "dew_point"
ATTR_WEATHER_HUMIDITY = "humidity"
ATTR_WEATHER_PRESSURE = "air_pressure"
ATTR_WEATHER_TEMPERATURE = "temperature"
ATTR_WEATHER_UV_INDEX = "uv_index"
ATTR_WEATHER_WIND_BEARING = "wind_bearing"
ATTR_WEATHER_WIND_GUST_SPEED = "wind_gust_speed"
ATTR_WEATHER_WIND_SPEED = "wind_speed"

# Weather conditions
ATTR_CONDITION_CLEAR_NIGHT = "clear-night"
ATTR_CONDITION_CLOUDY = "cloudy"
ATTR_CONDITION_EXCEPTIONAL = "exceptional"
ATTR_CONDITION_FOG = "fog"
ATTR_CONDITION_HAIL = "hail"
ATTR_CONDITION_LIGHTNING = "lightning"
ATTR_CONDITION_LIGHTNING_RAINY = "lightning-rainy"
ATTR_CONDITION_PARTLYCLOUDY = "partlycloudy"
ATTR_CONDITION_POURING = "pouring"
ATTR_CONDITION_RAINY = "rainy"
ATTR_CONDITION_SNOWY = "snowy"
ATTR_CONDITION_SNOWY_RAINY = "snowy-rainy"
ATTR_CONDITION_SUNNY = "sunny"
ATTR_CONDITION_WINDY = "windy"
ATTR_CONDITION_WINDY_VARIANT = "windy-variant"


class WeatherEntityFeature(IntFlag):
    """Weather entity features."""

    FORECAST_DAILY = 1
    FORECAST_HOURLY = 2
    FORECAST_TWICE_DAILY = 4


class Forecast(TypedDict, total=False):
    """Typed weather forecast dict."""

    condition: str | None
    datetime: Required[str]
    humidity: float | None
    precipitation_probability: int | None
    cloud_coverage: int | None
    native_precipitation: float | None
    native_pressure: float | None
    native_temperature: float | None
    native_templow: float | None
    native_apparent_temperature: float | None
    wind_bearing: float | str | None
    native_wind_gust_speed: float | None
    native_wind_speed: float | None
    native_dew_point: float | None
    uv_index: float | None


T = TypeVar("T", bound=DataUpdateCoordinator)


class SingleCoordinatorWeatherEntity(CoordinatorEntity[T], Generic[T]):
    """Weather entity using a single coordinator."""

    _attr_supported_features: int = 0
    _attr_native_temperature_unit: str | None = None
    _attr_native_precipitation_unit: str | None = None
    _attr_native_pressure_unit: str | None = None
    _attr_native_wind_speed_unit: str | None = None
    _attr_attribution: str | None = None
    _attr_has_entity_name: bool = False
    _attr_unique_id: str | None = None
    _attr_device_info: Any = None
    _attr_name: str | None = None

    def __init__(self, coordinator: T):
        """Initialize weather entity."""
        super().__init__(coordinator)

    @property
    def supported_features(self) -> int:
        """Return supported features."""
        return self._attr_supported_features

    @property
    def condition(self) -> str | None:
        """Return current condition."""
        return None

    @property
    def native_temperature(self) -> float | None:
        """Return temperature in native units."""
        return None

    @property
    def native_pressure(self) -> float | None:
        """Return pressure in native units."""
        return None

    @property
    def humidity(self) -> float | None:
        """Return humidity."""
        return None

    @property
    def native_wind_speed(self) -> float | None:
        """Return wind speed in native units."""
        return None

    @property
    def wind_bearing(self) -> float | str | None:
        """Return wind bearing."""
        return None

    @property
    def native_wind_gust_speed(self) -> float | None:
        """Return wind gust speed in native units."""
        return None

    @property
    def cloud_coverage(self) -> float | None:
        """Return cloud coverage."""
        return None

    @property
    def native_dew_point(self) -> float | None:
        """Return dew point in native units."""
        return None

    @property
    def uv_index(self) -> float | None:
        """Return UV index."""
        return None

    async def async_forecast_daily(self) -> list[Forecast] | None:
        """Return daily forecast."""
        return None

    async def async_forecast_hourly(self) -> list[Forecast] | None:
        """Return hourly forecast."""
        return None
