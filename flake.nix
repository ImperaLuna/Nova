{
  description = "Nova: a desktop shell for Wayland compositors";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # Rust sources only, so UI edits don't rebuild the daemon. Tokens.qml is
      # the exception: a novad test checks it declares every theme token.
      rustSrc =
        lib:
        lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            ./rustfmt.toml
            ./crates
            ./features
            ./packaging
            ./themes
            ./ui/theme/Tokens.qml
          ];
        };

      # The Quickshell releases Nova is tested with. flake.lock pins one exact
      # nixpkgs Quickshell; the quickshell-version check keeps it in this range.
      quickshellRange = builtins.fromTOML (builtins.readFile ./packaging/quickshell.toml);

      qmlImports = pkgs: [
        "${pkgs.quickshell}/lib/qt-6/qml"
        "${pkgs.qt6.qtdeclarative}/lib/qt-6/qml"
      ];
    in
    {
      packages = forAllSystems (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.nova;

        # The nova binary (CLI and daemon), feature manifests, the QML UI and
        # the systemd unit.
        nova = pkgs.rustPlatform.buildRustPackage {
          pname = "nova";
          version = "0.1.0";
          src = rustSrc pkgs.lib;
          cargoLock.lockFile = ./Cargo.lock;
          # Compiled in, so `nova` finds its UI with or without systemd.
          env.NOVA_UI_LAUNCHER = pkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.nova-ui;
          postInstall = ''
            mkdir -p $out/share/nova
            cp -r features $out/share/nova/features
            cp -r themes $out/share/nova/themes
            cp -r ${./ui} $out/share/nova/ui
            install -Dm644 packaging/systemd/nova.service.in $out/lib/systemd/user/nova.service
            substituteInPlace $out/lib/systemd/user/nova.service \
              --replace-fail @nova@ $out/bin/nova
          '';
          meta = {
            description = "Nova desktop shell";
            license = pkgs.lib.licenses.mit;
            mainProgram = "nova";
          };
        };

        nova-ui = pkgs.callPackage ./packaging/nix/nova-ui.nix { };
      });

      checks = forAllSystems (
        pkgs:
        let
          src = rustSrc pkgs.lib;
          cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
          cargoCheck =
            name: nativeBuildInputs: command:
            pkgs.stdenv.mkDerivation {
              inherit name src cargoDeps;
              nativeBuildInputs = [
                pkgs.rustPlatform.cargoSetupHook
                pkgs.cargo
                pkgs.rustc
              ]
              ++ nativeBuildInputs;
              buildPhase = command;
              installPhase = "touch $out";
            };
        in
        {
          # `nix build` runs `cargo test` as part of the package.
          build = self.packages.${pkgs.stdenv.hostPlatform.system}.nova;

          clippy = cargoCheck "nova-clippy" [ pkgs.clippy ] ''
            cargo clippy --workspace --all-targets --offline -- -D warnings
          '';

          rustfmt = cargoCheck "nova-rustfmt" [ pkgs.rustfmt ] ''
            cargo fmt --all --check
          '';

          qmllint = pkgs.runCommand "nova-qmllint" { nativeBuildInputs = [ pkgs.qt6.qtdeclarative ]; } ''
            cp -r ${./ui} ui && cp -r ${./scripts} scripts && chmod -R u+w .
            patchShebangs scripts
            scripts/qmllint.sh ${pkgs.lib.escapeShellArgs (qmlImports pkgs)}
            touch $out
          '';

          qmlformat = pkgs.runCommand "nova-qmlformat" { nativeBuildInputs = [ pkgs.qt6.qtdeclarative ]; } ''
            cp -r ${./ui} ui && cp -r ${./scripts} scripts && chmod -R u+w .
            patchShebangs scripts
            scripts/qmlformat.sh --check
            touch $out
          '';

          nix-lint =
            pkgs.runCommand "nova-nix-lint"
              {
                nativeBuildInputs = [
                  pkgs.nixfmt
                  pkgs.statix
                  pkgs.deadnix
                ];
              }
              ''
                cd ${./.}
                nixfmt --check $(find . -name '*.nix')
                statix check .
                deadnix --fail .
                touch $out
              '';

          quickshell-version =
            let
              inherit (pkgs.quickshell) version;
              inherit (quickshellRange) min max;
              # An `x` in max matches any number there (see the range file).
              comparableMax = builtins.replaceStrings [ "x" ] [ "999999" ] max;
              inRange = pkgs.lib.versionAtLeast version min && pkgs.lib.versionAtLeast comparableMax version;
            in
            pkgs.runCommand "nova-quickshell-version" { } (
              if inRange then
                "touch $out"
              else
                ''
                  echo "nixpkgs' Quickshell ${version} is outside the range Nova is tested with (${min} to ${max})." >&2
                  echo "Test Nova on it, then update packaging/quickshell.toml." >&2
                  exit 1
                ''
            );

          qml-tests =
            pkgs.runCommand "nova-qml-tests"
              {
                nativeBuildInputs = [ pkgs.qt6.qtdeclarative ];
                QT_QPA_PLATFORM = "offscreen";
                QML_IMPORT_PATH = "${pkgs.qt6.qtdeclarative}/lib/qt-6/qml";
              }
              ''
                export HOME=$TMPDIR
                qmltestrunner -input ${./ui}/tests
                touch $out
              '';
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
            pkgs.quickshell
            # So dev runs show the UI as nova-ui too; the daemon finds it on PATH.
            self.packages.${pkgs.stdenv.hostPlatform.system}.nova-ui
            pkgs.qt6.qtdeclarative # qmllint, qmlls, qmlformat, qmltestrunner
            pkgs.sway # headless compositor for e2e
            pkgs.grim
            pkgs.cargo-insta # `cargo insta review` for snapshot tests
            pkgs.nixfmt
            pkgs.statix
            pkgs.deadnix
            pkgs.just
            pkgs.jq
            pkgs.git
          ];
          # Lets `just lint-qml` and qmlls find Quickshell's types.
          NOVA_QML_IMPORTS = builtins.concatStringsSep ":" (qmlImports pkgs);
          QML_IMPORT_PATH = builtins.concatStringsSep ":" (qmlImports pkgs);
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
