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

        nodejs = pkgs.nodejs_22;

        app = pkgs.buildNpmPackage {
          pname = "librus-notifications";
          version = "1.0.0";

          src = ./.;

          npmDepsHash = "sha256-i8NjN9b29j7BaG7inBRLGUYDZGphMarKGO0R4LmdGmc=";

          makeCacheWritable = true;
          dontNpmBuild = true;

          inherit nodejs;

          nativeBuildInputs = with pkgs; [
            python3
            pkg-config
          ];

          buildInputs = with pkgs; [
            nodejs
          ];

          installPhase = ''
            runHook preInstall

            mkdir -p $out/bin
            mkdir -p $out/lib/librus-notifications

            cp -r src $out/lib/librus-notifications/
            cp index.js $out/lib/librus-notifications/
            cp librus-test.js $out/lib/librus-notifications/
            cp package.json $out/lib/librus-notifications/
            cp -r node_modules $out/lib/librus-notifications/

            cat > $out/bin/librus-notifications << EOF
#!/bin/sh
cd $out/lib/librus-notifications
exec ${nodejs}/bin/node index.js "\$@"
EOF
            chmod +x $out/bin/librus-notifications

            cat > $out/bin/librus-test << EOF
#!/bin/sh
cd $out/lib/librus-notifications
exec ${nodejs}/bin/node librus-test.js "\$@"
EOF
            chmod +x $out/bin/librus-test

            runHook postInstall
          '';

          meta = with pkgs.lib; {
            description = "Librus notifications service with AI summarization";
            license = licenses.mit;
            platforms = platforms.linux;
          };
        };

        dockerImage = pkgs.dockerTools.buildLayeredImage {
          name = "ghcr.io/flakm/czujka-librus";
          tag = "latest";

          contents = [ app pkgs.coreutils ];

          config = {
            Cmd = [ "${app}/bin/librus-notifications" ];
            WorkingDir = "/data";
            Env = [
              "NODE_ENV=production"
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
            nodejs
            pkgs.python3
            pkgs.pkg-config
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
