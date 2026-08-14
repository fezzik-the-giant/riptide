{
  description = "A terminal UI music player for Tidal, built with Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    crane,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {inherit system overlays;};

        rustToolchain = pkgs.rust-bin.stable.latest.default;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        cargoToml = fromTOML (builtins.readFile ./Cargo.toml);

        commonArgs = {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;

          src = craneLib.cleanCargoSource ./.;

          nativeBuildInputs = [pkgs.pkg-config];

          buildInputs =
            [
              pkgs.mpv
              pkgs.openssl
              pkgs.dbus
              pkgs.chafa
              pkgs.glib
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.CoreFoundation
            ];

          doCheck = false;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        riptide = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            meta = with pkgs.lib; {
              description = cargoToml.package.description;
              mainProgram = "riptide";
              license = licenses.gpl3Only;
              platforms = platforms.unix;
            };
          }
        );
      in {
        packages = {
          default = riptide;
          riptide = riptide;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = riptide;
          name = "riptide";
        };

        devShells.default = craneLib.devShell {
          inputsFrom = [riptide];
          packages = [
            rustToolchain
            pkgs.rust-analyzer
            pkgs.cargo-edit
            pkgs.cava
          ];
        };

        formatter = pkgs.alejandra;
      }
    );
}
