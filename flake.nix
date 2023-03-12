{
  description = "uncommitted-rs";

  inputs = {
    nixpkgs.url = "nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, utils, naersk }: {
    overlays = rec {
      expects-naersk = final: _: {
        uncommitted-rs = final.naersk.buildPackage {
          pname = "uncommitted-rs";
          root = ./.;
        };
      };

      default = _: prev: {
        inherit (prev.appendOverlays [
          naersk.overlay
          expects-naersk
        ]) uncommitted-rs;
      };
    };
  } // utils.lib.eachDefaultSystem (system: with import nixpkgs
    { overlays = [ self.overlays.default ]; inherit system; }; {
    packages.default = uncommitted-rs;

    devShells.default = mkShell {
      packages = [
        cargo
        cargo-watch
        rust-analyzer
        rustc
        rustfmt
      ];
    };
  });
}
