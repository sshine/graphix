{ inputs, ... }:
{
  flake.overlays.default = final: _prev: {
    graphix = inputs.self.packages.${final.stdenv.hostPlatform.system}.default;
  };
}
