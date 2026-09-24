{
  description = "gws-rust: Google Workspace CLI (gwsr) with a dynamic command surface from the Discovery Service";

  inputs = {
    # Deliberately pinned to the latest stable NixOS release branch rather than nixos-unstable, so
    # `nix flake update` only brings in security and bug-fix updates. Bump the branch when a new
    # NixOS release ships (Dependabot does not track flake inputs).
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    # Supplies the exact Rust toolchain pinned in rust-toolchain.toml.
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
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );

      workspace = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      cliCrate = builtins.fromTOML (builtins.readFile ./crates/gws-rust/Cargo.toml);

      toolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      gwsrFor =
        pkgs:
        let
          toolchain = toolchainFor pkgs;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        rustPlatform.buildRustPackage {
          pname = "gws-rust";
          inherit (workspace.workspace.package) version;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./crates
              # Read by drift tests (env table, CI skill-marker check).
              ./README.md
              ./.github/workflows/ci.yml
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.pkg-config ];
          # The HTTP client loads the platform CA store when it is built, even
          # for the loopback mock servers the tests use; the sandbox has none.
          nativeCheckInputs = [ pkgs.cacert ];

          cargoBuildFlags = [
            "--package"
            "gws-rust"
          ];
          # Run the whole workspace test suite; the tests are hermetic (no network).
          doCheck = true;
          cargoTestFlags = [ "--workspace" ];
          preCheck = ''
            export HOME="$(mktemp -d)"
            export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          '';

          meta = {
            inherit (cliCrate.package) description;
            homepage = workspace.workspace.package.repository;
            license = lib.licenses.asl20;
            mainProgram = "gwsr";
            platforms = systems;
          };
        };
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          gwsr = gwsrFor pkgs;
        in
        {
          default = gwsr;
          inherit gwsr;
        }
      );

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.gwsr;
          meta.description = "Run gwsr";
        };
      });

      checks = forAllSystems (pkgs: {
        gwsr = self.packages.${pkgs.stdenv.hostPlatform.system}.gwsr;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.gwsr ];
          packages = [
            ((toolchainFor pkgs).override {
              extensions = [
                "clippy"
                "rustfmt"
                "rust-src"
                "rust-analyzer"
                "llvm-tools-preview"
              ];
            })
            pkgs.actionlint
            pkgs.cargo-audit
            pkgs.cargo-cyclonedx
            pkgs.cargo-deny
            pkgs.cargo-llvm-cov
            pkgs.cargo-machete
            pkgs.just
            pkgs.lefthook
            pkgs.nodejs_24
            pkgs.pnpm
            pkgs.shellcheck
            pkgs.uv
            pkgs.zizmor
          ];
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
