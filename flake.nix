{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    mozillapkgs = {
      url = "github:mozilla/nixpkgs-mozilla";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, naersk, mozillapkgs }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages."${system}";

        mozilla = pkgs.callPackage (mozillapkgs + "/package-set.nix") { };
        rustChannel = mozilla.rustChannelOf {
          date = "2022-04-06";
          channel = "nightly";
          sha256 = "vOGzOgpFAWqSlXEs9IgMG7jWwhsmouGHSRHwAcHyccs=";
        };
        inherit (rustChannel) rust;

        naersk-lib = naersk.lib."${system}".override {
          cargo = rust;
          rustc = rust;
        };
      in
      rec {
        packages.default = naersk-lib.buildPackage {
          pname = "uncommitted";
          root = ./.;

          nativeBuildInputs = [ pkgs.pkg-config pkgs.openssl ];
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ rust pkgs.rust-analyzer pkgs.pkg-config pkgs.openssl ];
          shellHook = ''
            export RUST_SRC_PATH="${rustChannel.rust-src}/lib/rustlib/src/rust/library"
          '';
        };
      });
}
