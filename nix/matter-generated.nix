# A Rust file generated from the Matter data model, and the check that keeps
# the committed copy current.
#
# `generated` runs `script` over connectedhomeip's data model and formats the
# result with the repository's rustfmt, so a fresh copy is byte-identical to
# what `nix fmt` would produce. `check` diffs it against the committed file
# and, when they differ, prints the `cp` that brings the tree up to date.
{ pkgs
, connectedhomeip
, specVersion
, rustfmt
, rustfmtConfig
, src
, name
, script
, file
}:
let
  committedPath = "crates/hearthd/src/matter/${file}";

  generated = pkgs.runCommand name
    {
      nativeBuildInputs = [ pkgs.python3 rustfmt ];
    } ''
    mkdir -p $out
    python3 ${script} \
      ${connectedhomeip}/data_model/${specVersion} \
      $out/${file}
    rustfmt --config-path ${rustfmtConfig} $out/${file}
  '';

  check = pkgs.runCommand "${name}-check" { } ''
    if ! diff -u ${src}/${committedPath} ${generated}/${file}; then
      echo
      echo "${committedPath} is out of date. Refresh it with:"
      echo
      echo "  cp ${generated}/${file} ${committedPath}"
      echo
      exit 1
    fi
    touch $out
  '';
in
{
  inherit generated check;
}
