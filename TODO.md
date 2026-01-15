# hearthd Home Assistant Integration Support - TODO

## Goal
Build support for running Home Assistant integrations directly in hearthd via a sandboxed Python environment that communicates with Rust over Unix sockets.

**First Target**: `met` (Norwegian Meteorological Institute) weather integration

## Current Status
🚧 **Phase 1: Project Structure & Rust Foundation** - In Progress

---

## Phase 1: Project Structure & Rust Foundation
**Goal**: Set up the crate structure and basic Unix socket communication

- [ ] Create `crates/ha-protocol/` crate
  - [ ] Define message types (StateUpdate, EntityRegister, ServiceCall, etc.)
  - [ ] Implement JSON serialization/deserialization
  - [ ] Add Cargo.toml with serde dependencies
- [ ] Create `crates/ha-sandbox/` crate
  - [ ] Implement Unix socket server
  - [ ] Handle Python process lifecycle
  - [ ] Add basic sandbox configuration
  - [ ] Implement message routing
- [ ] Create `crates/ha-runtime/` crate
  - [ ] Integration state management
  - [ ] Entity registry
  - [ ] Event coordination
- [ ] Update workspace Cargo.toml to include new crates
- [ ] Update hearthd main crate to integrate new components

---

## Phase 2: Python Shim Library Foundation
**Goal**: Create minimal Python stubs that can load and communicate

- [ ] Set up Python development environment
  - [ ] Create `python/.venv/` virtual environment
  - [ ] Create `python/requirements.txt`
  - [ ] Install dependencies: PyMetno, aiohttp, voluptuous, etc.
- [ ] Create `python/homeassistant-shim/` package
  - [ ] Set up package structure with `__init__.py`
  - [ ] Create `pyproject.toml`
  - [ ] Add to .gitignore: `.venv/`, `*.pyc`, `__pycache__/`
- [ ] Implement `homeassistant/core.py`
  - [ ] `HomeAssistant` class with socket client
  - [ ] Event bus stub
  - [ ] State machine stub (sends states to Rust)
  - [ ] Service registry stub
  - [ ] Config object
- [ ] Implement `homeassistant/const.py`
  - [ ] Platform enum
  - [ ] Common constants
- [ ] Test basic socket communication
  - [ ] Python can connect to Rust socket
  - [ ] Can send/receive simple messages

---

## Phase 3: Entity & Platform System
**Goal**: Support entity registration and state management

- [ ] Implement `homeassistant/helpers/entity.py`
  - [ ] `Entity` base class
  - [ ] State property and attributes
  - [ ] `async_write_ha_state()` → sends to socket
  - [ ] Entity lifecycle methods
- [ ] Implement `homeassistant/helpers/entity_platform.py`
  - [ ] `EntityPlatform` class
  - [ ] Entity registration
  - [ ] Platform setup/teardown
  - [ ] `async_add_entities` callback
- [ ] Implement `homeassistant/helpers/update_coordinator.py`
  - [ ] `DataUpdateCoordinator` class
  - [ ] Periodic refresh scheduling
  - [ ] Error handling
  - [ ] Listener management
  - [ ] `async_config_entry_first_refresh()`
- [ ] Implement weather platform support
  - [ ] Weather entity base class
  - [ ] Weather-specific attributes

---

## Phase 4: Integration Loader
**Goal**: Load and initialize the `met` integration

- [ ] Implement `homeassistant/loader.py`
  - [ ] `Integration` class
  - [ ] Manifest.json parsing
  - [ ] Dependency validation (fail if PyMetno missing)
  - [ ] Component loading via importlib
  - [ ] Platform discovery
- [ ] Implement `homeassistant/helpers/config_entries.py`
  - [ ] `ConfigEntry` class
  - [ ] Entry lifecycle (setup/unload)
  - [ ] Runtime data storage
  - [ ] `async_forward_entry_setups()`
- [ ] Copy met integration for testing
  - [ ] Create `python/integrations/met/`
  - [ ] Copy all files from HA core
  - [ ] Verify manifest.json is present
- [ ] Test integration loading
  - [ ] Can discover met integration
  - [ ] Can parse manifest
  - [ ] Can import __init__.py

---

## Phase 5: Met Integration Support
**Goal**: Get `met` integration fully working

- [ ] Implement coordinator support for met
  - [ ] Handle Met.no API calls
  - [ ] Parse weather data
  - [ ] Update entities on refresh
- [ ] Implement config entry setup
  - [ ] Support latitude/longitude config
  - [ ] Support track_home option
  - [ ] Create weather entity
- [ ] Test against real Met.no API
  - [ ] Can fetch weather data
  - [ ] Entity has correct state
  - [ ] Attributes populated correctly
- [ ] Compare with real Home Assistant
  - [ ] Same state values
  - [ ] Same attribute structure
  - [ ] Same update intervals

---

## Phase 6: Testing & Validation
**Goal**: Ensure correctness and stability

- [ ] Create integration test harness
  - [ ] Mock Met.no API for deterministic tests
  - [ ] Test setup flow
  - [ ] Test data updates
  - [ ] Test teardown
- [ ] Error handling tests
  - [ ] Network failures
  - [ ] Invalid coordinates
  - [ ] Missing dependencies
  - [ ] Malformed responses
- [ ] Performance testing
  - [ ] Memory usage in sandbox
  - [ ] CPU usage during updates
  - [ ] Socket message throughput

---

## Phase 7: Documentation
**Goal**: Document architecture and usage

- [ ] Write `docs/architecture.md`
  - [ ] System overview diagram
  - [ ] Component responsibilities
  - [ ] Data flow
  - [ ] Design decisions
- [ ] Write `docs/protocol.md`
  - [ ] Message types specification
  - [ ] Request/response patterns
  - [ ] Error handling
  - [ ] Example messages
- [ ] Create example configuration
  - [ ] How to enable met integration
  - [ ] Configuration options
  - [ ] Troubleshooting common issues
- [ ] Write development guide
  - [ ] How to add new integrations
  - [ ] Testing procedures
  - [ ] Debugging tips

---

## Success Criteria

The implementation is complete when:
1. ✅ hearthd can load the `met` integration in a sandbox
2. ✅ Integration can be configured via config entry
3. ✅ Weather entity appears with current conditions
4. ✅ Data updates every 30 minutes (coordinator polling)
5. ✅ Entity states match real Home Assistant output
6. ✅ Graceful failure if dependencies missing
7. ✅ Full documentation of architecture and protocol

---

## Notes

- Using Python venv for prototyping (easier than Nix for initial development)
- Nix packaging can be added later once the design is validated
- Focus on getting `met` working first before expanding to other integrations
- Keep the socket protocol simple initially (JSON over Unix domain socket)
