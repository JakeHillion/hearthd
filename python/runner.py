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
from typing import Any, Dict, Optional


class SocketTransport:
    """Handles newline-delimited JSON communication over a Unix socket."""

    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.reader: Optional[asyncio.StreamReader] = None
        self.writer: Optional[asyncio.StreamWriter] = None

    async def connect(self):
        """Initialize asyncio streams from the socket."""
        self.reader, self.writer = await asyncio.open_unix_connection(sock=self.sock)
        logging.debug("Socket transport connected")

    async def send_message(self, message: Dict[str, Any]) -> None:
        """Send a message to Rust (newline-delimited JSON)."""
        if not self.writer:
            raise RuntimeError("Transport not connected")

        json_str = json.dumps(message)
        logging.getLogger().log(5, f"Sending: {json_str}")  # TRACE level = 5

        self.writer.write(json_str.encode() + b"\n")
        await self.writer.drain()

    async def recv_response(self) -> Dict[str, Any]:
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

    def __init__(self, entry_id: str, transport: SocketTransport):
        self.entry_id = entry_id
        self.transport = transport
        self.running = True

    async def send_ready(self):
        """Send Ready message to indicate we're initialized."""
        await self.transport.send_message({"type": "ready"})
        logging.info(f"[{self.entry_id}] Sent Ready message")

    async def handle_setup_integration(self, domain: str, entry_id: str, config: Dict[str, Any]):
        """Handle SetupIntegration response from Rust."""
        logging.info(f"[{entry_id}] Setting up integration: {domain}")
        logging.debug(f"[{entry_id}] Config: {config}")

        try:
            # Import the integration module
            module_name = f"homeassistant.components.{domain}"
            logging.info(f"[{entry_id}] Importing {module_name}")

            try:
                integration_module = importlib.import_module(module_name)
            except ModuleNotFoundError as e:
                # Check if this is a missing dependency or missing integration
                error_msg = str(e)
                if "No module named" in error_msg:
                    # Extract the missing module name
                    missing_module = error_msg.split("'")[1] if "'" in error_msg else "unknown"

                    # Check if it's the integration itself or a dependency
                    if missing_module == module_name or missing_module == domain:
                        # Integration doesn't exist
                        error_type = "integration_not_found"
                        error_detail = f"Integration '{domain}' not found in Home Assistant source"
                    else:
                        # Missing Python dependency
                        error_type = "missing_dependency"
                        error_detail = f"Integration '{domain}' requires Python package '{missing_module}' which is not installed"

                    logging.error(f"[{entry_id}] {error_detail}")
                    await self.transport.send_message({
                        "type": "setup_failed",
                        "entry_id": entry_id,
                        "error": error_detail,
                        "error_type": error_type,
                        "missing_package": missing_module
                    })
                    return
                else:
                    raise
            except ImportError as e:
                # Other import errors
                logging.error(f"[{entry_id}] Import error: {e}", exc_info=True)
                await self.transport.send_message({
                    "type": "setup_failed",
                    "entry_id": entry_id,
                    "error": f"Failed to import integration '{domain}': {e}",
                    "error_type": "import_error"
                })
                return

            # Check if async_setup_entry exists
            if not hasattr(integration_module, "async_setup_entry"):
                error_detail = f"Integration '{domain}' has no async_setup_entry function"
                logging.error(f"[{entry_id}] {error_detail}")
                await self.transport.send_message({
                    "type": "setup_failed",
                    "entry_id": entry_id,
                    "error": error_detail,
                    "error_type": "invalid_integration"
                })
                return

            logging.info(f"[{entry_id}] Successfully imported {domain} integration")

            # Create HomeAssistant instance
            from homeassistant.core import HomeAssistant, ConfigEntry
            from homeassistant.helpers.entity_platform import AddEntitiesCallback

            # TODO: Get socket FD from self to pass to HomeAssistant
            hass = HomeAssistant()
            hass._reader = self.transport.reader
            hass._writer = self.transport.writer
            hass._send_message = self.transport.send_message
            hass._recv_message = self.transport.recv_response

            # Create ConfigEntry
            config_entry = ConfigEntry(
                entry_id=entry_id,
                domain=domain,
                data=config
            )

            # Call async_setup_entry
            logging.info(f"[{entry_id}] Calling async_setup_entry for {domain}")

            # For platforms that support async_setup_entry signature with async_add_entities
            setup_result = await integration_module.async_setup_entry(hass, config_entry)

            if setup_result is False:
                error_detail = f"Integration '{domain}' async_setup_entry returned False"
                logging.error(f"[{entry_id}] {error_detail}")
                await self.transport.send_message({
                    "type": "setup_failed",
                    "entry_id": entry_id,
                    "error": error_detail,
                    "error_type": "setup_failed"
                })
                return

            logging.info(f"[{entry_id}] Integration setup complete")

            # Send setup complete
            await self.transport.send_message({
                "type": "setup_complete",
                "entry_id": entry_id,
                "platforms": []  # TODO: Extract platforms from forward_entry_setups
            })

        except Exception as e:
            logging.error(f"[{entry_id}] Setup failed: {e}", exc_info=True)
            await self.transport.send_message({
                "type": "setup_failed",
                "entry_id": entry_id,
                "error": str(e),
                "error_type": "unknown"
            })

    async def handle_response(self, response: Dict[str, Any]):
        """Handle a response message from Rust."""
        msg_type = response.get("type")

        if msg_type == "setup_integration":
            await self.handle_setup_integration(
                response["domain"],
                response["entry_id"],
                response["config"]
            )

        elif msg_type == "unload_integration":
            entry_id = response["entry_id"]
            logging.info(f"[{entry_id}] Unloading integration")
            # TODO: Implement unload
            await self.transport.send_message({
                "type": "unload_complete",
                "entry_id": entry_id
            })

        elif msg_type == "trigger_update":
            timer_id = response["timer_id"]
            entry_id = response["entry_id"]
            logging.debug(f"[{entry_id}] Timer {timer_id} triggered")
            # TODO: Trigger coordinator update
            await self.transport.send_message({
                "type": "update_complete",
                "timer_id": timer_id,
                "success": True
            })

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
        logging.info(f"[{self.entry_id}] Integration runner starting")

        # Send Ready message
        await self.send_ready()

        # Message loop
        try:
            while self.running:
                response = await self.transport.recv_response()
                await self.handle_response(response)

        except EOFError:
            logging.info("Socket closed by peer")
        except Exception as e:
            logging.error(f"Runner error: {e}", exc_info=True)
        finally:
            await self.transport.close()
            logging.info(f"[{self.entry_id}] Integration runner stopped")


def setup_logging(entry_id: str):
    """Configure logging for this sandbox."""
    # Add TRACE level (below DEBUG)
    TRACE_LEVEL = 5
    logging.addLevelName(TRACE_LEVEL, "TRACE")

    def trace(self, message, *args, **kwargs):
        if self.isEnabledFor(TRACE_LEVEL):
            self._log(TRACE_LEVEL, message, args, **kwargs)

    logging.Logger.trace = trace  # type: ignore

    # Configure logging
    log_format = f"[%(asctime)s] [%(levelname)s] [{entry_id}] %(message)s"
    logging.basicConfig(
        level=logging.DEBUG,
        format=log_format,
        datefmt="%Y-%m-%d %H:%M:%S"
    )

    logging.info(f"Logging configured for sandbox {entry_id}")


async def main():
    """Entry point for the Python integration runner."""
    # Get configuration from environment
    socket_fd_str = os.environ.get("HEARTHD_SOCKET_FD")
    entry_id = os.environ.get("HEARTHD_ENTRY_ID", "unknown")
    ha_source_path = os.environ.get("HEARTHD_HA_SOURCE")

    if not socket_fd_str:
        print("ERROR: HEARTHD_SOCKET_FD not set", file=sys.stderr)
        sys.exit(1)

    if not ha_source_path:
        print("ERROR: HEARTHD_HA_SOURCE not set", file=sys.stderr)
        sys.exit(1)

    # Add paths to sys.path in the correct order:
    # 1. Our shims go first (higher priority for homeassistant.core, etc.)
    shim_path = os.path.join(os.path.dirname(__file__), "homeassistant-shim")
    if shim_path not in sys.path:
        sys.path.insert(0, shim_path)
        print(f"Added shim path {shim_path} to sys.path", file=sys.stderr)

    # 2. HA source goes second (for homeassistant.components.*)
    if ha_source_path not in sys.path:
        sys.path.append(ha_source_path)
        print(f"Added HA source {ha_source_path} to sys.path", file=sys.stderr)

    # Setup logging
    setup_logging(entry_id)

    try:
        socket_fd = int(socket_fd_str)
        logging.info(f"Starting runner with socket FD: {socket_fd}")

        # Create socket from file descriptor
        sock = socket.fromfd(socket_fd, socket.AF_UNIX, socket.SOCK_STREAM)

        # Close the original FD (socket object owns it now)
        os.close(socket_fd)

        # Create transport and runner
        transport = SocketTransport(sock)
        await transport.connect()

        runner = IntegrationRunner(entry_id, transport)
        await runner.run()

    except Exception as e:
        logging.error(f"Fatal error: {e}", exc_info=True)
        sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
