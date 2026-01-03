"""aiohttp client helper that proxies through Rust."""

import asyncio
import json
import logging
import uuid
from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from homeassistant.core import HomeAssistant

_LOGGER = logging.getLogger(__name__)


class ClientSession:
    """HTTP client session that proxies requests through Rust."""

    def __init__(self, hass: "HomeAssistant"):
        self.hass = hass
        self._pending_requests: dict[str, asyncio.Future] = {}

    async def _send_request(
        self,
        method: str,
        url: str,
        headers: dict | None = None,
        data: bytes | None = None,
        timeout: float = 30.0,
    ) -> "ClientResponse":
        """Send HTTP request via Rust proxy."""
        request_id = str(uuid.uuid4())

        # Create a future to wait for the response
        future: asyncio.Future = asyncio.get_event_loop().create_future()
        self._pending_requests[request_id] = future

        # Send HTTP request message to Rust
        msg = {
            "type": "http_request",
            "request_id": request_id,
            "method": method.upper(),
            "url": url,
            "headers": headers or {},
            "timeout_ms": int(timeout * 1000),
        }
        if data:
            # Convert bytes to list for JSON serialization
            msg["body"] = list(data)

        _LOGGER.debug("Sending HTTP request: %s %s", method, url)
        await self.hass._send_message(msg)

        try:
            # Wait for the response with timeout
            response_data = await asyncio.wait_for(future, timeout=timeout + 5)
            return ClientResponse(
                status=response_data.get("status", 0),
                body=response_data.get("body", b""),
                headers=response_data.get("headers", {}),
                error=response_data.get("error"),
            )
        except asyncio.TimeoutError:
            self._pending_requests.pop(request_id, None)
            raise
        except Exception as e:
            self._pending_requests.pop(request_id, None)
            raise RuntimeError(f"HTTP request failed: {e}") from e

    def handle_http_response(self, response_data: dict) -> None:
        """Handle HTTP response from Rust (called by runner)."""
        request_id = response_data.get("request_id")
        if request_id and request_id in self._pending_requests:
            future = self._pending_requests.pop(request_id)
            if not future.done():
                # Convert body from list of ints back to bytes
                body = response_data.get("body", [])
                if isinstance(body, list):
                    body = bytes(body)
                elif isinstance(body, str):
                    body = body.encode()
                response_data["body"] = body
                future.set_result(response_data)

    async def get(self, url: str, **kwargs) -> "ClientResponse":
        """Send GET request via Rust proxy."""
        return await self._send_request(
            "GET",
            url,
            headers=kwargs.get("headers"),
            timeout=kwargs.get("timeout", 30.0),
        )

    async def post(self, url: str, **kwargs) -> "ClientResponse":
        """Send POST request via Rust proxy."""
        data = kwargs.get("data")
        if isinstance(data, str):
            data = data.encode()
        return await self._send_request(
            "POST",
            url,
            headers=kwargs.get("headers"),
            data=data,
            timeout=kwargs.get("timeout", 30.0),
        )

    async def close(self):
        """Close the session."""
        # Cancel any pending requests
        for future in self._pending_requests.values():
            if not future.done():
                future.cancel()
        self._pending_requests.clear()


class ClientResponse:
    """HTTP response."""

    def __init__(
        self,
        status: int,
        body: bytes,
        headers: dict[str, str],
        error: str | None = None,
    ):
        self.status = status
        self._body = body
        self.headers = headers
        self._error = error

    @property
    def ok(self) -> bool:
        """Return True if status is 2xx."""
        return 200 <= self.status < 300

    async def text(self) -> str:
        """Get response text."""
        if isinstance(self._body, bytes):
            return self._body.decode("utf-8", errors="replace")
        return str(self._body)

    async def json(self) -> Any:
        """Get response JSON."""
        text = await self.text()
        return json.loads(text)

    async def read(self) -> bytes:
        """Read response bytes."""
        if isinstance(self._body, bytes):
            return self._body
        return self._body.encode()

    def raise_for_status(self) -> None:
        """Raise exception if status is not ok."""
        if self._error:
            raise RuntimeError(self._error)
        if not self.ok:
            raise RuntimeError(f"HTTP error {self.status}")


def async_get_clientsession(hass: "HomeAssistant") -> ClientSession:
    """Get aiohttp client session."""
    # Reuse existing session if available
    if "aiohttp_session" not in hass.data:
        hass.data["aiohttp_session"] = ClientSession(hass)
    return hass.data["aiohttp_session"]
