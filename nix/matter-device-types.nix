# The generated `DeviceType` enum and the check that keeps the committed copy
# current.
#
# `generated` runs the generator over connectedhomeip's data model and formats
# the result with the repository's rustfmt, so a fresh copy is byte-identical
# to what `nix fmt` would produce. `check` diffs it against the committed
# file and, when they differ, prints the `cp` that brings the tree up to date.
{ pkgs
, connectedhomeip
, specVersion
, rustfmt
, rustfmtConfig
, src
}:
let
  committedPath = "crates/hearthd/src/matter/device_types.rs";

  generated = pkgs.runCommand "matter-device-types"
    {
      nativeBuildInputs = [ pkgs.python3 rustfmt ];
    } ''
    mkdir -p $out
    python3 ${./matter-device-types.py} \
      ${connectedhomeip}/data_model/${specVersion} \
      $out/device_types.rs
    rustfmt --config-path ${rustfmtConfig} $out/device_types.rs
  '';

  check = pkgs.runCommand "matter-device-types-check" { } ''
    if ! diff -u ${src}/${committedPath} ${generated}/device_types.rs; then
      echo
      echo "${committedPath} is out of date. Refresh it with:"
      echo
      echo "  cp ${generated}/device_types.rs ${committedPath}"
      echo
      exit 1
    fi
    touch $out
  '';
in
{
  inherit generated check;
}
