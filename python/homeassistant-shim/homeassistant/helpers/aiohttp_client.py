"""aiohttp client helper that proxies through Rust."""

import asyncio
import json
from typing import Any

from homeassistant.core import HomeAssistant


class ClientSession:
    """HTTP client session that proxies requests through Rust."""

    def __init__(self, hass: HomeAssistant):
        self.hass = hass

    async def get(self, url: str, **kwargs) -> "ClientResponse":
        """Send GET request via Rust proxy."""
        # Send HTTP request message to Rust
        await self.hass._send_message({
            "type": "http_request",
            "method": "GET",
            "url": url,
            "headers": kwargs.get("headers", {}),
        })

        # Wait for HTTP response from Rust
        response_msg = await self.hass._recv_message()

        if response_msg and response_msg.get("type") == "http_response":
            return ClientResponse(
                status=response_msg.get("status", 200),
                body=response_msg.get("body", ""),
                headers=response_msg.get("headers", {}),
            )

        raise RuntimeError("Failed to get HTTP response from Rust")

    async def close(self):
        """Close the session."""
        pass


class ClientResponse:
    """HTTP response."""

    def __init__(self, status: int, body: str, headers: dict[str, str]):
        self.status = status
        self._body = body
        self.headers = headers

    async def text(self) -> str:
        """Get response text."""
        return self._body

    async def json(self) -> Any:
        """Get response JSON."""
        return json.loads(self._body)

    async def read(self) -> bytes:
        """Read response bytes."""
        return self._body.encode()


def async_get_clientsession(hass: HomeAssistant) -> ClientSession:
    """Get aiohttp client session."""
    # Reuse existing session if available
    if "aiohttp_session" not in hass.data:
        hass.data["aiohttp_session"] = ClientSession(hass)
    return hass.data["aiohttp_session"]
