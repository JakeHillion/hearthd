"""Weather component shim for hearthd."""

import logging
from datetime import datetime, timezone
from enum import IntFlag
from typing import Any, Generic, Required, TypedDict, TypeVar

from homeassistant.helpers.update_coordinator import (
    CoordinatorEntity,
    DataUpdateCoordinator,
)

_LOGGER = logging.getLogger(__name__)

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

    async def async_send_state_to_rust(self) -> None:
        """Send current weather state to Rust engine."""
        hass = getattr(self, 'hass', None)
        if hass is None or not hasattr(hass, '_send_message'):
            return

        uid = getattr(self, '_attr_unique_id', None)
        try:
            uid = self.unique_id or uid
        except Exception:
            pass
        uid = uid or 'unknown'
        entity_id = f"weather.{uid}"

        # Get condition safely (may depend on hass.sun)
        condition = None
        try:
            condition = self.condition
        except Exception:
            _LOGGER.debug("Could not get condition", exc_info=True)

        attrs: dict[str, Any] = {
            "condition": condition,
            "temperature": self.native_temperature,
            "humidity": self.humidity,
            "pressure": self.native_pressure,
            "wind_speed": self.native_wind_speed,
            "wind_bearing": self.wind_bearing,
            "wind_gust": self.native_wind_gust_speed,
            "cloud_coverage": self.cloud_coverage,
            "dew_point": self.native_dew_point,
            "uv_index": self.uv_index,
        }

        # Get forecasts - Met.no uses sync @callback methods _async_forecast_*
        # as well as the async versions
        try:
            if self.supported_features & WeatherEntityFeature.FORECAST_DAILY:
                daily = None
                # Try sync _async_forecast_daily first (Met.no pattern)
                sync_fn = getattr(self, '_async_forecast_daily', None)
                if sync_fn is not None:
                    daily = sync_fn()
                else:
                    daily = await self.async_forecast_daily()
                if daily:
                    attrs["forecast_daily"] = daily
        except Exception:
            _LOGGER.debug("Could not get daily forecast", exc_info=True)

        try:
            if self.supported_features & WeatherEntityFeature.FORECAST_HOURLY:
                hourly = None
                sync_fn = getattr(self, '_async_forecast_hourly', None)
                if sync_fn is not None:
                    hourly = sync_fn()
                else:
                    hourly = await self.async_forecast_hourly()
                if hourly:
                    attrs["forecast_hourly"] = hourly
        except Exception:
            _LOGGER.debug("Could not get hourly forecast", exc_info=True)

        await hass._send_message({
            "type": "state_update",
            "entity_id": entity_id,
            "state": condition or "unknown",
            "attributes": attrs,
            "last_updated": datetime.now(timezone.utc).isoformat(),
        })
