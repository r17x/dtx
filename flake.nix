{
  description = "dtx - Dev Tools eXperience: Process orchestration control plane with Nix";

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      imports = [ ];

      perSystem =
        {
          pkgs,
          lib,
          system,
          ...
        }:
        let
          # Apply rust-overlay to get consistent Rust toolchain
          rustPkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };

          # Rust toolchain with all components (consistent versions)
          # Using stable.latest (1.85+) - required for edition2024 dependencies
          # Minimum supported: 1.75+ as specified in docs (CLAUDE.md, AGENTS.md, DESIGN.md)
          rustToolchain = rustPkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "clippy"
              "rustfmt"
            ];
          };
          # Build the dtx binary using crane for better git dependency handling
          craneLib = inputs.crane.mkLib pkgs;

          # Common arguments for crane builds
          commonArgs = {
            pname = "dtx";
            version = "0.0.1";
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs =
              with pkgs;
              [
                sqlite
                openssl
              ]
              ++ lib.optionals stdenv.hostPlatform.isDarwin [
                libiconv
              ];

            # Required for SQLx compile-time verification
            DATABASE_URL = "sqlite::memory:";
            SQLX_OFFLINE = "true";
          };

          # Build dependencies separately for caching
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          # Build the dtx package
          dtx = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;

              # Run tests
              doCheck = true;

              meta = with lib; {
                description = "Dev Tools eXperience - Control plane for process-compose with Nix integration";
                homepage = "https://github.com/r17x/dtx";
                license = with licenses; [
                  mit
                  asl20
                ];
                maintainers = [ ];
                mainProgram = "dtx";
              };
            }
          );

          # Clippy check
          dtxClippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
            }
          );

          # Format check
          dtxFmt = craneLib.cargoFmt {
            src = ./.;
            pname = "dtx";
          };

          # Doc check
          dtxDoc = craneLib.cargoDoc (commonArgs // { inherit cargoArtifacts; });
        in
        {
          # ===================================
          # Packages
          # ===================================
          packages = {
            inherit dtx;
            default = dtx;
          };

          # ===================================
          # Apps (for `nix run`)
          # ===================================
          apps = {
            default = {
              type = "app";
              program = "${dtx}/bin/dtx";
            };
            dtx = {
              type = "app";
              program = "${dtx}/bin/dtx";
            };
          };

          # ===================================
          # Checks (for `nix flake check`)
          # ===================================
          checks = {
            inherit dtx dtxClippy dtxFmt dtxDoc;
          };

          # ===================================
          # Development Shell
          # ===================================
          devShells.default = pkgs.mkShell {
            inputsFrom = [ dtx ];

            packages = with pkgs; [
              # ===================================
              # Rust Toolchain (from rust-overlay for consistent versions)
              # ===================================
              rustToolchain

              # Rust development tools
              cargo-watch # Auto-rebuild on changes
              cargo-edit # cargo add, cargo rm, cargo upgrade
              cargo-outdated # Check for outdated dependencies
              cargo-audit # Security audit
              cargo-expand # Expand macros
              cargo-flamegraph # Performance profiling
              cargo-tarpaulin # Code coverage

              # ===================================
              # Database Tools
              # ===================================
              sqlx-cli # SQLx database migrations & query verification
              sqlite # SQLite database (embedded)
 # PostgreSQL (for testing)
              redis # Redis (for testing)

              # ===================================
              # Process Management
              # ===================================
              bun
              process-compose # The tool we're building a control plane for

              # ===================================
              # Nix Tools
              # ===================================
              nix # Nix package manager
              nil # Nix language server
              nixpkgs-fmt # Nix code formatter

              # ===================================
              # Agentic Tooling
              # ===================================
              inputs.serena.packages.${system}.default

              # ===================================
              # General Development Tools
              # ===================================
              git
              jq # JSON processor
              yq-go # YAML processor
              curl
              ripgrep # Fast grep (rg)
              fd # Fast find
              tokei # Line counter
              hyperfine # Benchmarking
              nodejs

              # ===================================
              # Documentation
              # ===================================
              mdbook # Documentation generator (for future)
            ];

            shellHook = ''
              # Setup environment
              export ROOT_REPO=$(git rev-parse --show-toplevel 2>/dev/null || echo "$PWD")
              export DATA_DIR="$ROOT_REPO/.data"
              export PROJECT_ROOT="$ROOT_REPO"
              mkdir -p "$DATA_DIR"/{postgres,redis,sqlite,logs}

              export PATH="$ROOT_REPO/.data/target/release:$PATH"
              export DATABASE_URL="sqlite:$DATA_DIR/sqlite/dtx.db"
              export PGDATA="$DATA_DIR/postgres"
              export REDIS_DATA="$DATA_DIR/redis"
              export RUST_BACKTRACE=1
              export RUST_LOG=info
              export SQLX_OFFLINE=false

              # Log verbose info to file (see: cat .data/shellhook.log)
              {
                echo "=== dtx shell initialized: $(date) ==="
                echo ""
                echo "Rust: $(rustc --version)"
                echo "Cargo: $(cargo --version)"
                echo "SQLx: $(sqlx --version)"
                echo "SQLite: $(sqlite3 --version)"
                echo "PostgreSQL: $(postgres --version | head -1)"
                echo "Redis: $(redis-server --version)"
                echo "Process-compose: $(process-compose version 2>/dev/null | head -1 || echo 'available')"
                echo ""
                echo "DATABASE_URL=$DATABASE_URL"
                echo "PGDATA=$PGDATA"
                echo "ROOT_REPO=$ROOT_REPO"
              } > "$DATA_DIR/shellhook.log" 2>&1

              # Minimal user output
              echo "🦀 dtx dev shell ready | rustc $(rustc --version | cut -d' ' -f2) | cat .data/shellhook.log for details"
            '';
          };

          # ===================================
          # Formatter (for `nix fmt`)
          # ===================================
          formatter = pkgs.nixpkgs-fmt;
        };

      # ===================================
      # Flake-wide outputs (overlays)
      # ===================================
      flake = {
        # Overlay for adding dtx to other flakes
        overlays.default = final: prev: {
          dtx = final.callPackage (
            { crane, ... }:
            let
              craneLib = crane.mkLib final;
              commonArgs = {
                src = craneLib.cleanCargoSource ./.;
                strictDeps = true;
                nativeBuildInputs = [ final.pkg-config ];
                buildInputs =
                  [
                    final.sqlite
                    final.openssl
                  ]
                  ++ final.lib.optionals final.stdenv.hostPlatform.isDarwin [
                    final.libiconv
                  ];
                DATABASE_URL = "sqlite::memory:";
                SQLX_OFFLINE = "true";
              };
              cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            in
            craneLib.buildPackage (
              commonArgs
              // {
                inherit cargoArtifacts;
                meta = {
                  description = "Dev Tools eXperience - Control plane for process-compose with Nix integration";
                  homepage = "https://github.com/r17x/dtx";
                  license = with final.lib.licenses; [
                    mit
                    asl20
                  ];
                  mainProgram = "dtx";
                };
              }
            )
          ) { crane = inputs.crane; };
        };

        # NixOS module (optional, for system-wide installation)
        nixosModules.default =
          { pkgs, ... }:
          {
            environment.systemPackages = [ pkgs.dtx ];
          };
      };
    };

  inputs = {
    # Flake utilities
    flake-parts.url = "github:hercules-ci/flake-parts";

    # Nixpkgs
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixpkgs.follows = "nixpkgs-unstable";

    # Crane for Rust builds (better git dependency handling)
    crane.url = "github:ipetkov/crane";

    # Rust toolchain overlay (consistent rustc + rust-analyzer versions)
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Agentic tooling
    llms-agents.url = "github:numtide/llm-agents.nix";
    serena.url = "github:oraios/serena";
    serena.inputs.nixpkgs.follows = "nixpkgs";
  };
}
