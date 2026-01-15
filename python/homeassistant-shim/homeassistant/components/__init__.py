"""Components namespace package.

This merges shimmed components with vendor components by extending __path__.
"""

import sys
import os

# Extend __path__ to include vendor's components directory
# This makes Python search both locations when importing submodules
for path in sys.path:
    if 'vendor/ha-core' in path or path.endswith('ha-core'):
        vendor_components = os.path.join(path, 'homeassistant', 'components')
        if os.path.isdir(vendor_components) and vendor_components not in __path__:
            __path__.append(vendor_components)
            break
