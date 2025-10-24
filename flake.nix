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
      in
      {
        packages.default = pkgs.buildNpmPackage {
          pname = "librus-notifications";
          version = "1.0.0";

          src = ./.;

          npmDepsHash = "sha256-56/Mbia20JUGteAq67xU4KGtRAfOpd0zL/uOhudHjII=";

          makeCacheWritable = true;
          dontNpmBuild = true;

          nativeBuildInputs = with pkgs; [
            python3
            pkg-config
            pkgs.nodejs_20.pkgs.node-gyp
          ];

          buildInputs = with pkgs; [
            nodejs_20
            pkgs.gcc
            pkgs.stdenv.cc
          ];

          installPhase = ''
            mkdir -p $out/bin
            mkdir -p $out/lib/librus-notifications

            cp -r src $out/lib/librus-notifications/
            cp index.js $out/lib/librus-notifications/
            cp package.json $out/lib/librus-notifications/
            cp package-lock.json $out/lib/librus-notifications/

            cd $out/lib/librus-notifications

            # Install dependencies with proper native module rebuilding
            export npm_config_nodedir=${pkgs.nodejs_20}
            export npm_config_node_gyp=${pkgs.nodejs_20.pkgs.node-gyp}/lib/node_modules/node-gyp/bin/node-gyp.js
            HOME=$TMPDIR ${pkgs.nodejs_20}/bin/npm ci --omit=dev --build-from-source

            cat > $out/bin/librus-notifications << EOF
            #!/bin/sh
            cd $out/lib/librus-notifications
            exec ${pkgs.nodejs_20}/bin/node index.js "\$@"
            EOF

            chmod +x $out/bin/librus-notifications
          '';

          meta = with pkgs.lib; {
            description = "Librus notifications service with AI summarization";
            license = licenses.mit;
            platforms = platforms.linux;
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            nodejs_20
            python3
            pkg-config
          ];

          shellHook = ''
            echo "Librus Notifications Development Environment"
            echo "Run 'npm install' to install dependencies"
          '';
        };
      }
    ) // {
      nixosModules.default = nixosModule;
    };
}
