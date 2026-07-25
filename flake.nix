{
  description = "Product-neutral bounded child-process execution";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      eachSystem = function: nixpkgs.lib.genAttrs systems (system: function system);
    in
    {
      checks = eachSystem (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        {
          cargo-test = rustPlatform.buildRustPackage {
            pname = "bounded-exec-tests";
            version = "0.1.0";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "--workspace" ];
            cargoTestFlags = [ "--workspace" ];
            installPhase = ''
              touch "$out"
            '';
          };

          cargo-clippy = rustPlatform.buildRustPackage {
            pname = "bounded-exec-clippy";
            version = "0.1.0";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            buildPhase = ''
              cargo clippy --workspace --all-targets --offline -- -D warnings
            '';
            doCheck = false;
            installPhase = ''
              touch "$out"
            '';
          };

          core-no-std-wasm = rustPlatform.buildRustPackage {
            pname = "bounded-exec-core-no-std-wasm";
            version = "0.1.0";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [
              "-p"
              "bounded-exec-core"
              "--target"
              "wasm32-unknown-unknown"
            ];
            doCheck = false;
            installPhase = ''
              touch "$out"
            '';
          };

          cargo-format =
            pkgs.runCommand "bounded-exec-format"
              {
                nativeBuildInputs = [ toolchain ];
                src = self;
              }
              ''
                cd "$src"
                cargo fmt --all -- --check
                touch "$out"
              '';

          radicle-publication-receipt =
            pkgs.runCommand "bounded-exec-radicle-publication-receipt"
              {
                nativeBuildInputs = [
                  pkgs.b3sum
                  pkgs.coreutils
                  pkgs.diffutils
                  pkgs.gnugrep
                  pkgs.jq
                  pkgs.nickel
                ];
                src = self;
              }
              ''
                cd "$src"
                expected_hash="$(cat evidence/radicle/publication-v1.blake3)"
                actual_hash="$(b3sum evidence/radicle/publication-v1.json | cut -d ' ' -f 1)"
                test "$actual_hash" = "$expected_hash"

                nickel export --format json evidence/radicle/publication-v1.ncl \
                  | jq -S . > "$TMPDIR/publication-from-nickel.json"
                cmp evidence/radicle/publication-v1.json "$TMPDIR/publication-from-nickel.json"

                nickel eval tests/radicle-publication.ncl > "$TMPDIR/result.ncl"
                grep -q 'tests = true' "$TMPDIR/result.ncl"
                printf '%s\n' \
                  "Bounded Exec Radicle publication receipt accepted" \
                  "publication_receipt_blake3=$expected_hash" \
                  > "$out"
              '';
        }
      );

      devShells = eachSystem (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          default = pkgs.mkShell {
            packages = [
              toolchain
              pkgs.b3sum
              pkgs.jq
              pkgs.nickel
              pkgs.nixfmt
            ];
          };
        }
      );
    };
}
