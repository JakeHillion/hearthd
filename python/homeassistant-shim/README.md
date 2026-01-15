# homeassistant-shim

Home Assistant compatibility shim for hearthd.

This package provides stub implementations of Home Assistant's core modules,
allowing Home Assistant integrations to run within hearthd's sandboxed
environment while communicating with the Rust runtime via Unix sockets.

## Structure

```
homeassistant/
├── __init__.py
├── core.py              # HomeAssistant main class
├── const.py             # Constants
├── loader.py            # Integration loading
└── helpers/
    ├── entity.py        # Entity base class
    ├── entity_platform.py
    ├── update_coordinator.py
    └── config_entries.py
```

## Installation

This package is installed in development mode within hearthd's Python venv:

```bash
cd python
source .venv/bin/activate
pip install -e homeassistant-shim/
```
