#!/usr/bin/env python3
"""
hearthd Python integration runner.

This script is spawned by hearthd's sandbox.rs to run Home Assistant integrations.
It communicates with the Rust parent via a socketpair passed as a file descriptor.
"""

import asyncio
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
            # TODO: Dynamically import and setup integration
            # For now, just acknowledge
            logging.info(f"[{entry_id}] Integration setup not yet implemented")

            # Send setup complete
            await self.transport.send_message({
                "type": "setup_complete",
                "entry_id": entry_id,
                "platforms": []  # TODO: Get from integration
            })

        except Exception as e:
            logging.error(f"[{entry_id}] Setup failed: {e}", exc_info=True)
            await self.transport.send_message({
                "type": "setup_failed",
                "entry_id": entry_id,
                "error": str(e)
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

    if not socket_fd_str:
        print("ERROR: HEARTHD_SOCKET_FD not set", file=sys.stderr)
        sys.exit(1)

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
