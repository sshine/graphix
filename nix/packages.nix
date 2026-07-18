# Workspace packages via buildRustPackage.
{ ... }:
{
  perSystem =
    { pkgs, lib, ... }:
    let
      buildCrate =
        {
          pname,
          mainProgram ? pname,
        }:
        pkgs.rustPlatform.buildRustPackage {
          inherit pname;
          version = "0.1.0";
          src = ../.;
          cargoLock.lockFile = ../Cargo.lock;
          buildAndTestSubdir = "crates/${pname}";
          meta.mainProgram = mainProgram;
        };
    in
    rec {
      packages = {
        default = buildCrate { pname = "graphix"; };
        conventional-commit-check = buildCrate { pname = "conventional-commit-check"; };
      };

      checks.graphix = packages.default;

      apps.default = {
        type = "app";
        program = lib.getExe packages.default;
      };
    };
}
