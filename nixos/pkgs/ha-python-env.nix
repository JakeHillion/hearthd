{ python313 }:

# Python environment with Home Assistant dependencies
# This provides the runtime dependencies needed for HA integrations
python313.withPackages (ps: with ps; [
  # Core HA shim dependencies
  aiohttp
  voluptuous
  python-dateutil

  # Integration-specific packages
  pymetno # Met.no weather integration

  # Common HA component dependencies
  orjson
  aiozoneinfo
  python-slugify
  xmltodict
])
