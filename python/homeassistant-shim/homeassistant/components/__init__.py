"""Components namespace package.

hearthd's shim replaces Home Assistant's core modules but not its components,
which are imported unmodified from a real Home Assistant source tree. Extending
``__path__`` is what lets ``homeassistant.components.met`` resolve there while
``homeassistant.core`` still resolves to the shim.

The source tree's location is passed in ``HEARTHD_HA_SOURCE`` by the Rust side,
which resolved it from configuration or from the path baked in at build time.
It is read directly rather than recovered by searching ``sys.path`` for a
recognisable-looking entry: the tree can live anywhere, including a read-only
store path with no telltale name.
"""

import os

_source = os.environ.get("HEARTHD_HA_SOURCE")
if _source:
    _components = os.path.join(_source, "homeassistant", "components")
    if os.path.isdir(_components) and _components not in __path__:
        __path__.append(_components)
