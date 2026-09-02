#!/usr/bin/env python3
"""
hearthd Python integration runner.

This script is spawned by hearthd's sandbox.rs to run Home Assistant integrations.
It communicates with the Rust parent via a socketpair passed as a file descriptor.
"""

import asyncio
import importlib
import json
import logging
import os
import socket
import sys
from typing import Any


class SocketTransport:
    """Handles newline-delimited JSON communication over a Unix socket."""

    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.reader: asyncio.StreamReader | None = None
        self.writer: asyncio.StreamWriter | None = None

    async def connect(self):
        """Initialize asyncio streams from the socket."""
        self.reader, self.writer = await asyncio.open_unix_connection(sock=self.sock)
        logging.debug("Socket transport connected")

    async def send_message(self, message: dict[str, Any]) -> None:
        """Send a message to Rust (newline-delimited JSON)."""
        if not self.writer:
            raise RuntimeError("Transport not connected")

        json_str = json.dumps(message)
        logging.getLogger().log(5, f"Sending: {json_str}")  # TRACE level = 5

        self.writer.write(json_str.encode() + b"\n")
        await self.writer.drain()

    async def recv_response(self) -> dict[str, Any]:
        """Receive a response from Rust (newline-delimited JSON)."""
        if not self.reader:
            raise RuntimeError("Transport not connected")

        line = await self.reader.readline()
        if not line:
            raise EOFError("Socket closed")

        json_str = line.decode().strip()
        logging.getLogger().log(5, f"Received: {json_str}")  # TRACE level = 5

        return json.loads(json_str)

    async def close(self):
        """Close the transport."""
        if self.writer:
            self.writer.close()
            await self.writer.wait_closed()


class IntegrationRunner:
    """Main runner for Home Assistant integrations."""

    def __init__(self, name: str, transport: SocketTransport):
        self.name = name
        self.transport = transport
        self.running = True
        self.hass = None  # Will be set during setup

    async def send_ready(self):
        """Send Ready message to indicate we're initialized."""
        await self.transport.send_message({"type": "ready"})
        logging.info(f"[{self.name}] Sent Ready message")

    async def handle_setup_integration(
        self, domain: str, name: str, config: dict[str, Any]
    ):
        """Handle SetupIntegration response from Rust."""
        logging.info(f"[{name}] Setting up integration: {domain}")
        logging.debug(f"[{name}] Config: {config}")

        try:
            # Import the integration module
            module_name = f"homeassistant.components.{domain}"
            logging.info(f"[{name}] Importing {module_name}")
            logging.debug(f"[{name}] sys.path: {sys.path[:3]}")

            try:
                integration_module = importlib.import_module(module_name)
            except ModuleNotFoundError as e:
                # Check if this is a missing dependency or missing integration
                error_msg = str(e)
                if "No module named" in error_msg:
                    # Extract the missing module name
                    missing_module = (
                        error_msg.split("'")[1] if "'" in error_msg else "unknown"
                    )

                    # Check if it's the integration itself or a dependency
                    if missing_module in (module_name, domain):
                        # Integration doesn't exist
                        error_type = "integration_not_found"
                        error_detail = (
                            f"Integration '{domain}' not found in Home Assistant source"
                        )
                    else:
                        # Missing Python dependency
                        error_type = "missing_dependency"
                        error_detail = f"Integration '{domain}' requires Python package '{missing_module}' which is not installed"

                    logging.error(f"[{name}] {error_detail}")
                    await self.transport.send_message(
                        {
                            "type": "setup_failed",
                            "name": name,
                            "error": error_detail,
                            "error_type": error_type,
                            "missing_package": missing_module,
                        }
                    )
                    return
                else:
                    raise
            except ImportError as e:
                # Other import errors
                logging.error(f"[{name}] Import error: {e}", exc_info=True)
                await self.transport.send_message(
                    {
                        "type": "setup_failed",
                        "name": name,
                        "error": f"Failed to import integration '{domain}': {e}",
                        "error_type": "import_error",
                    }
                )
                return

            # Check if async_setup_entry exists
            if not hasattr(integration_module, "async_setup_entry"):
                error_detail = (
                    f"Integration '{domain}' has no async_setup_entry function"
                )
                logging.error(f"[{name}] {error_detail}")
                await self.transport.send_message(
                    {
                        "type": "setup_failed",
                        "name": name,
                        "error": error_detail,
                        "error_type": "invalid_integration",
                    }
                )
                return

            logging.info(f"[{name}] Successfully imported {domain} integration")

            # Create HomeAssistant instance
            from homeassistant.core import ConfigEntry
            from homeassistant.core import HomeAssistant

            # Create and configure HomeAssistant instance
            hass = HomeAssistant()
            hass._reader = self.transport.reader
            hass._writer = self.transport.writer
            hass._send_message = self.transport.send_message
            hass._recv_message = self.transport.recv_response
            self.hass = hass  # Store for TriggerUpdate handling

            # Create ConfigEntry
            config_entry = ConfigEntry(entry_id=name, domain=domain, data=config)

            # Call async_setup_entry
            logging.info(f"[{name}] Calling async_setup_entry for {domain}")

            # For platforms that support async_setup_entry signature with async_add_entities
            setup_result = await integration_module.async_setup_entry(
                hass, config_entry
            )

            if setup_result is False:
                error_detail = (
                    f"Integration '{domain}' async_setup_entry returned False"
                )
                logging.error(f"[{name}] {error_detail}")
                await self.transport.send_message(
                    {
                        "type": "setup_failed",
                        "name": name,
                        "error": error_detail,
                        "error_type": "setup_failed",
                    }
                )
                return

            logging.info(f"[{name}] Integration setup complete")

            # Send setup complete
            await self.transport.send_message(
                {
                    "type": "setup_complete",
                    "name": name,
                    "platforms": [],  # TODO: Extract platforms from forward_entry_setups
                }
            )

        except Exception as e:
            logging.error(f"[{name}] Setup failed: {e}", exc_info=True)
            await self.transport.send_message(
                {
                    "type": "setup_failed",
                    "name": name,
                    "error": str(e),
                    "error_type": "unknown",
                }
            )

    async def handle_response(self, response: dict[str, Any]):
        """Handle a response message from Rust."""
        msg_type = response.get("type")

        if msg_type == "setup_integration":
            await self.handle_setup_integration(
                response["domain"], response["name"], response["config"]
            )

        elif msg_type == "unload_integration":
            name = response["name"]
            logging.info(f"[{name}] Unloading integration")
            # TODO: Implement unload
            await self.transport.send_message({"type": "unload_complete", "name": name})

        elif msg_type == "trigger_update":
            timer_id = response["timer_id"]
            name = response["name"]
            logging.debug(f"[{name}] Timer {timer_id} triggered")

            # Look up coordinator and trigger refresh
            if self.hass and timer_id in self.hass._coordinators:
                coordinator = self.hass._coordinators[timer_id]
                try:
                    await coordinator.async_refresh()
                    await self.transport.send_message(
                        {
                            "type": "update_complete",
                            "timer_id": timer_id,
                            "success": True,
                        }
                    )
                except Exception as e:
                    logging.error(
                        f"[{name}] Coordinator refresh failed: {e}", exc_info=True
                    )
                    await self.transport.send_message(
                        {
                            "type": "update_complete",
                            "timer_id": timer_id,
                            "success": False,
                            "error": str(e),
                        }
                    )
            else:
                logging.warning(f"[{name}] No coordinator found for timer {timer_id}")
                await self.transport.send_message(
                    {
                        "type": "update_complete",
                        "timer_id": timer_id,
                        "success": False,
                        "error": f"No coordinator found for timer {timer_id}",
                    }
                )

        elif msg_type == "shutdown":
            logging.info("Received shutdown signal")
            self.running = False

        elif msg_type == "ack":
            # Acknowledgment, no action needed
            pass

        elif msg_type == "error":
            logging.error(f"Rust error: {response.get('message')}")

        else:
            logging.warning(f"Unknown response type: {msg_type}")

    async def run(self):
        """Main message loop."""
        logging.info(f"[{self.name}] Integration runner starting")

        # Send Ready message
        await self.send_ready()

        # Start background message receiver
        receiver_task = asyncio.create_task(self._message_receiver())

        # Wait for receiver to complete
        try:
            await receiver_task
        except asyncio.CancelledError:
            logging.info("Receiver task cancelled")
        except Exception as e:
            logging.error(f"Runner error: {e}", exc_info=True)
        finally:
            await self.transport.close()
            logging.info(f"[{self.name}] Integration runner stopped")

    async def _message_receiver(self):
        """Background task to receive and dispatch messages."""
        # Dispatch tasks are held for as long as they run. asyncio keeps only a
        # weak reference to a running task, so one that nothing else holds can
        # be collected mid-flight and simply never finish.
        dispatching: set[asyncio.Task] = set()
        try:
            while self.running:
                response = await self.transport.recv_response()
                # Dispatch without blocking the receive loop.
                task = asyncio.create_task(self._dispatch_message(response))
                dispatching.add(task)
                task.add_done_callback(dispatching.discard)

        except EOFError:
            logging.info("Socket closed by peer")
        except Exception as e:
            logging.error(f"Receiver error: {e}", exc_info=True)
            raise

    async def _dispatch_message(self, response: dict[str, Any]):
        """Dispatch a message to the appropriate handler."""
        try:
            await self.handle_response(response)
        except Exception as e:
            logging.error(f"Error handling message: {e}", exc_info=True)


def setup_logging(name: str):
    """Configure logging for this sandbox."""
    # Add TRACE level (below DEBUG)
    TRACE_LEVEL = 5
    logging.addLevelName(TRACE_LEVEL, "TRACE")

    def trace(self, message, *args, **kwargs):
        if self.isEnabledFor(TRACE_LEVEL):
            self._log(TRACE_LEVEL, message, args, **kwargs)

    logging.Logger.trace = trace  # type: ignore

    # Configure logging
    log_format = f"[%(asctime)s] [%(levelname)s] [{name}] %(message)s"
    logging.basicConfig(
        level=logging.DEBUG, format=log_format, datefmt="%Y-%m-%d %H:%M:%S"
    )

    logging.info(f"Logging configured for sandbox {name}")


def configure_environment() -> tuple[int, str]:
    """Read the runner's environment and assemble ``sys.path``.

    Synchronous, and done before the event loop starts: this is startup wiring,
    not work the loop should ever be blocked on. Returns the socket descriptor
    and the name this sandbox logs under.
    """
    socket_fd_str = os.environ.get("HEARTHD_SOCKET_FD")
    name = os.environ.get("HEARTHD_NAME", "unknown")
    ha_source_path = os.environ.get("HEARTHD_HA_SOURCE")

    # Defaults to the shim beside this script, which is how it is laid out both
    # in the source tree and when packaged; the Rust side passes HEARTHD_SHIM
    # explicitly so the two never have to agree by coincidence.
    shim_path = os.environ.get("HEARTHD_SHIM") or os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "homeassistant-shim"
    )

    for var, value in (
        ("HEARTHD_SOCKET_FD", socket_fd_str),
        ("HEARTHD_HA_SOURCE", ha_source_path),
    ):
        if not value:
            print(f"ERROR: {var} not set", file=sys.stderr)
            sys.exit(1)

    # Order matters. The shim goes first so that homeassistant.core and friends
    # resolve to hearthd's replacements; the real source goes second, supplying
    # homeassistant.components.* through the __path__ extension in the shim's
    # components package.
    shim_path = os.path.abspath(shim_path)
    ha_source_path = os.path.abspath(ha_source_path)
    for entry in (shim_path, ha_source_path):
        if entry in sys.path:
            sys.path.remove(entry)
    sys.path.insert(0, shim_path)
    sys.path.append(ha_source_path)

    return int(socket_fd_str), name


async def main(socket_fd: int, name: str):
    """Entry point for the Python integration runner."""
    try:
        logging.info(f"Starting runner with socket FD: {socket_fd}")

        # Create socket from file descriptor
        sock = socket.fromfd(socket_fd, socket.AF_UNIX, socket.SOCK_STREAM)

        # Close the original FD (socket object owns it now)
        os.close(socket_fd)

        # Create transport and runner
        transport = SocketTransport(sock)
        await transport.connect()

        runner = IntegrationRunner(name, transport)
        await runner.run()

    except Exception as e:
        logging.error(f"Fatal error: {e}", exc_info=True)
        sys.exit(1)


if __name__ == "__main__":
    fd, sandbox_name = configure_environment()
    setup_logging(sandbox_name)
    asyncio.run(main(fd, sandbox_name))
