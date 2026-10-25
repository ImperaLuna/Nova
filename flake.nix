{
  description = "Nova: a desktop shell for Wayland compositors";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # RustSec's advisories for the cargo-deny check, which has no network.
    # Pinned in flake.lock like nixpkgs; `nix flake update advisory-db`
    # brings in new advisories, and the weekly "Bump advisory-db" workflow
    # opens a PR that does it.
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    # The newest stable Quickshell release, for the "Newest Quickshell"
    # workflow, which overrides this with the newest tag it finds; the tag
    # here is only a default. Not the Quickshell Nova ships: that is nixpkgs'.
    quickshell-newest = {
      url = "git+https://github.com/quickshell-mirror/quickshell?ref=refs/tags/v0.3.2&shallow=1";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      advisory-db,
      quickshell-newest,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # Rust sources only, so UI edits don't rebuild the daemon. Tokens.qml and
      # protocol.js are the exceptions: Rust tests check they match novad.
      rustSrc =
        lib:
        lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            ./deny.toml
            ./rustfmt.toml
            ./crates
            ./features
            ./packaging
            ./themes
            ./ui/theme/Tokens.qml
            ./ui/daemon/protocol.js
          ];
        };

      # The Quickshell releases Nova is tested with. flake.lock pins one exact
      # nixpkgs Quickshell; the quickshell-version check keeps it in this range.
      quickshellRange = builtins.fromTOML (builtins.readFile ./packaging/quickshell.toml);
      # Whether Quickshell `version` is in that range. An `x` in max matches
      # any number there (see the range file).
      inQuickshellRange =
        version:
        let
          inherit (quickshellRange) min max;
          comparableMax = builtins.replaceStrings [ "x" ] [ "999999" ] max;
        in
        nixpkgs.lib.versionAtLeast version min && nixpkgs.lib.versionAtLeast comparableMax version;

      # pkgs with Quickshell swapped for the quickshell-newest release, built
      # with nixpkgs' recipe. The same release as nixpkgs' keeps nixpkgs'
      # build, so the newest-Quickshell checks reuse CI's cached ones.
      withNewestQuickshell =
        pkgs:
        pkgs.extend (
          _: prev: {
            quickshell =
              let
                cmake = builtins.readFile "${quickshell-newest}/CMakeLists.txt";
                version = builtins.head (builtins.match ".*project\\(quickshell VERSION \"([^\"]+)\".*" cmake);
              in
              if version == prev.quickshell.version then
                prev.quickshell
              else
                prev.quickshell.overrideAttrs {
                  inherit version;
                  src = quickshell-newest;
                };
          }
        );

      inherit ((builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package) version;

      # share/nova: what nova finds next to its binary (nova-paths).
      installShare = ''
        mkdir -p $out/share/nova
        cp -r features $out/share/nova/features
        cp -r themes $out/share/nova/themes
        cp -r ${./ui} $out/share/nova/ui
      '';

      # The nova binary (CLI and daemon), feature manifests, the QML UI and
      # the systemd unit, on pkgs' Quickshell.
      novaFor =
        pkgs:
        let
          nova-ui = pkgs.callPackage ./packaging/nix/nova-ui.nix { };
        in
        {
          inherit nova-ui;
          nova = pkgs.rustPlatform.buildRustPackage {
            pname = "nova";
            inherit version;
            src = rustSrc pkgs.lib;
            cargoLock.lockFile = ./Cargo.lock;
            # setpriv, for the launcher test's app that dies with its parent.
            nativeCheckInputs = [ pkgs.util-linux ];
            # Compiled in, so `nova` finds its UI with or without systemd.
            env.NOVA_UI_LAUNCHER = pkgs.lib.getExe nova-ui;
            postInstall = installShare + ''
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
        };

      # The release archive install.sh fetches on other distros:
      # nova-<cpu>-linux.tar.gz with bin/nova (static, musl) and share/nova,
      # laid out like the prefix it unpacks into. Built without
      # NOVA_UI_LAUNCHER: there novad runs the distro's Quickshell from PATH.
      # Tests ran in the regular build; this one skips them.
      distFor =
        pkgs:
        let
          nova = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
            pname = "nova";
            inherit version;
            src = rustSrc pkgs.lib;
            cargoLock.lockFile = ./Cargo.lock;
            doCheck = false;
            postInstall = installShare;
          };
          name = "nova-${pkgs.stdenv.hostPlatform.parsed.cpu.name}-linux";
        in
        pkgs.runCommand "${name}-${version}" { } ''
          mkdir -p tree $out
          cp -r ${nova}/bin ${nova}/share tree/
          # Store files are read-only; an upgrade has to replace share/nova.
          chmod -R u+w tree
          tar -C tree --sort=name --owner=0 --group=0 --numeric-owner --mtime=@1 -c bin share \
            | gzip -n > $out/${name}.tar.gz
          (cd $out && sha256sum ${name}.tar.gz > ${name}.tar.gz.sha256)
        '';

      # qmllint over ui/ against pkgs' Quickshell types.
      qmllintFor =
        pkgs:
        pkgs.runCommand "nova-qmllint" { nativeBuildInputs = [ pkgs.qt6.qtdeclarative ]; } ''
          cp -r ${./ui} ui && cp -r ${./scripts} scripts && chmod -R u+w .
          patchShebangs scripts
          scripts/qmllint.sh ${pkgs.lib.escapeShellArgs (qmlImports pkgs)}
          touch $out
        '';

      qmlImports = pkgs: [
        "${pkgs.quickshell}/lib/qt-6/qml"
        "${pkgs.qt6.qtdeclarative}/lib/qt-6/qml"
      ];
    in
    {
      packages = forAllSystems (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.nova;

        inherit (novaFor pkgs) nova nova-ui;
        # .github/workflows/release.yml attaches this to a GitHub release.
        dist = distFor pkgs;
      });

      # Nova on the newest stable Quickshell release instead of nixpkgs': its
      # build, qmllint and VM tests, for .github/workflows/newest-quickshell.yml.
      # Not checks, so `nix flake check` doesn't build a second Quickshell.
      legacyPackages = forAllSystems (
        pkgs:
        let
          newest = withNewestQuickshell pkgs;
          inherit (novaFor newest) nova;
        in
        {
          newest-quickshell =
            import ./e2e/vm {
              pkgs = newest;
              inherit nova;
            }
            // {
              inherit (newest) quickshell;
              inherit nova;
              qmllint = qmllintFor newest;
              inRange = inQuickshellRange newest.quickshell.version;
            };
        }
      );

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
        # VM tests on niri and Hyprland (e2e/vm): e2e-niri, e2e-hyprland.
        import ./e2e/vm {
          inherit pkgs;
          inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) nova;
        }
        // {
          # `nix build` runs `cargo test` as part of the package.
          build = self.packages.${pkgs.stdenv.hostPlatform.system}.nova;
          # The release archive, so it can't break unnoticed before a tag.
          inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) dist;

          clippy = cargoCheck "nova-clippy" [ pkgs.clippy ] ''
            cargo clippy --workspace --all-targets --offline -- -D warnings
          '';

          # Advisories, licenses, duplicate crates and sources (deny.toml).
          # cargo-deny reads the advisory database from a git clone in the
          # folder it would clone the default URL to, and dates it by its
          # commit, so the pinned input becomes a one-commit repo there.
          cargo-deny =
            cargoCheck "nova-cargo-deny"
              [
                pkgs.cargo-deny
                pkgs.git
              ]
              ''
                export CARGO_HOME=$TMPDIR/cargo-home
                db=$CARGO_HOME/advisory-dbs/advisory-db-3157b0e258782691
                mkdir -p $db && cp -r ${advisory-db}/. $db && chmod -R u+w $db
                export GIT_AUTHOR_DATE=@${toString advisory-db.lastModified} GIT_COMMITTER_DATE=@${toString advisory-db.lastModified}
                git -C $db init -q && git -C $db add -A
                git -C $db -c user.name=nix -c user.email=nix@localhost commit -q -m advisory-db
                cargo deny --offline check
              '';

          rustfmt = cargoCheck "nova-rustfmt" [ pkgs.rustfmt ] ''
            cargo fmt --all --check
          '';

          qmllint = qmllintFor pkgs;

          # The committed .qsb shaders match their .frag sources.
          shaders = pkgs.runCommand "nova-shaders" { nativeBuildInputs = [ pkgs.qt6.qtshadertools ]; } ''
            cp -r ${./ui} ui && cp -r ${./scripts} scripts && chmod -R u+w .
            patchShebangs scripts
            scripts/shaders.sh --check
            touch $out
          '';

          # install.sh: shellcheck, and a run against a fake release.
          install-script =
            pkgs.runCommand "nova-install-script"
              {
                nativeBuildInputs = [
                  pkgs.shellcheck
                  pkgs.curl
                ];
              }
              ''
                cp ${./install.sh} install.sh && cp ${./scripts/test-install.sh} test-install.sh
                shellcheck install.sh test-install.sh
                patchShebangs test-install.sh
                ./test-install.sh install.sh
                touch $out
              '';

          # The blur rules `nova setup` writes for niri, against this niri.
          niri-rules = pkgs.runCommand "nova-niri-rules" { nativeBuildInputs = [ pkgs.niri ]; } ''
            niri validate -c ${./packaging/compositor/niri.kdl}
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
            in
            pkgs.runCommand "nova-quickshell-version" { } (
              if inQuickshellRange version then
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
            pkgs.qt6.qtshadertools # qsb, for scripts/shaders.sh
            pkgs.sway # headless compositor for e2e
            pkgs.grim
            pkgs.cargo-insta # `cargo insta review` for snapshot tests
            pkgs.cargo-deny
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
