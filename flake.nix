{
  description = "Librus notifications service with AI summarization";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      nixosModule = import ./module.nix;
    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        app = pkgs.rustPlatform.buildRustPackage {
          pname = "librus-notifications";
          version = "2.0.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            outputHashes = {
              "librus-rs-2.0.2" = "sha256-lgR8dj0Qt089i6+bh7pFKwNCwobGIOmO14Y3piMf6hY=";
            };
          };

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl ];

          doCheck = false;
        };

        dockerImage = pkgs.dockerTools.buildLayeredImage {
          name = "ghcr.io/flakm/czujka-librus";
          tag = "latest";

          contents = [ app pkgs.coreutils ];

          config = {
            Cmd = [ "${app}/bin/librus-notifications" ];
            WorkingDir = "/data";
            Env = [
              "DB_PATH=/data/librus.db"
            ];
            Volumes = {
              "/data" = {};
            };
          };
        };
      in
      {
        packages = {
          default = app;
          docker = dockerImage;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.rustc
            pkgs.cargo
            pkgs.rustfmt
            pkgs.clippy
            pkgs.stdenv.cc
          ];

          shellHook = ''
            echo "Librus Notifications Development Environment"
            echo "Run 'cargo build' to compile the Rust service"
          '';
        };
      }
    ) // {
      nixosModules.default = nixosModule;
    };
}
