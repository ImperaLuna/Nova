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
    # Builds the Rust dependencies as their own derivation, so a change to
    # Nova's code reuses them instead of compiling every crate again.
    crane.url = "github:ipetkov/crane";
    # The newest stable Quickshell release, for the "Newest Quickshell"
    # workflow, which overrides this with the newest tag it finds; the tag
    # here is only a default. Not the Quickshell Nova ships: that is nixpkgs'.
    quickshell-newest = {
      url = "git+https://github.com/quickshell-mirror/quickshell?ref=refs/tags/v0.3.2&shallow=1";
      flake = false;
    };
    # The standalone Nova Files app, which the `nova-files` feature installs.
    # Its own nixpkgs, not Nova's: following Nova's would change its store
    # paths and miss its binary cache, so every user would build GPUI.
    nova-files.url = "github:ImperaLuna/Nova-Files";
  };

  outputs =
    {
      self,
      nixpkgs,
      advisory-db,
      crane,
      quickshell-newest,
      nova-files,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # The UI side's crates, the ones that may use GPUI (AGENTS.md, Layout):
      # the nova-ui program, the shared UI parts and each feature's ui. The
      # daemon's sources below leave them out, so a change to them alone
      # rebuilds nothing of the daemon's.
      uiCrates =
        lib:
        lib.fileset.unions (
          [
            ./app-ui
            (lib.fileset.maybeMissing ./framework-ui)
          ]
          ++ map (id: lib.fileset.maybeMissing (./features + "/${id}/ui")) (
            builtins.attrNames (builtins.readDir ./features)
          )
        );

      # ./features without the ui crates: the manifests, and the daemon
      # side's crates.
      daemonFeatures = lib: lib.fileset.difference ./features (uiCrates lib);

      # What the UI side builds from: its crates, and the daemon side's that
      # they use by path (api and -info crates), with the root manifest
      # those inherit from, and the few other files its crates read.
      uiSrc =
        lib:
        lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.toml
            ./rustfmt.toml
            ./clippy.toml
            ./features
            ./framework
            (uiCrates lib)
            # The look kit compiles in the fallback theme, and its tests
            # hold its tokens and glyphs equal to the Quickshell shell's.
            ./themes
            ./ui/theme/Tokens.qml
            ./ui/theme/icons.js
          ];
        };

      # The daemon side's Rust sources only, so UI edits don't rebuild the
      # daemon. Tokens.qml and protocol.js are the exceptions: Rust tests
      # check they match novad.
      rustSrc =
        lib:
        lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            ./deny.toml
            ./rustfmt.toml
            # Clippy's limits and test exemptions.
            ./clippy.toml
            (daemonFeatures lib)
            # The crate folder kinds (AGENTS.md, Layout).
            ./framework
            ./fronts
            ./app
            ./packaging
            ./themes
            ./ui/theme/Tokens.qml
            ./ui/daemon/protocol.js
            # The slot hosts and view registry, checked against the manifests.
            ./ui/slots
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

      # The Rust workspace on pkgs, built with crane: cargoArtifacts holds the
      # compiled dependencies and only rebuilds when Cargo.lock changes.
      rustFor =
        pkgs:
        let
          craneLib = crane.mkLib pkgs;
          common = {
            pname = "nova";
            inherit version;
            src = rustSrc pkgs.lib;
            strictDeps = true;
          };
        in
        {
          inherit craneLib common;
          cargoArtifacts = craneLib.buildDepsOnly common;
        };

      # What the headless e2e reads from the checkout: its script, the UI it
      # runs and the feature manifests.
      e2eSrc =
        lib:
        lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./e2e/run.sh
            (daemonFeatures lib)
            ./ui
          ];
        };

      # share/nova under `prefix`: what nova finds next to its binary
      # (nova-paths). Copied in after the Rust build, so UI edits don't
      # recompile nova.
      installShare = lib: prefix: ''
        mkdir -p ${prefix}/share/nova
        cp -r --no-preserve=mode ${
          lib.fileset.toSource {
            root = ./features;
            fileset = daemonFeatures lib;
          }
        } ${prefix}/share/nova/features
        cp -r --no-preserve=mode ${./themes} ${prefix}/share/nova/themes
        cp -r --no-preserve=mode ${./ui} ${prefix}/share/nova/ui
      '';

      # The nova binary (CLI and daemon), feature manifests, the QML UI and
      # the systemd unit, on pkgs' Quickshell.
      novaFor =
        pkgs:
        let
          nova-quickshell = pkgs.callPackage ./packaging/nix/nova-quickshell.nix { };
          inherit (rustFor pkgs) craneLib common cargoArtifacts;
        in
        rec {
          inherit nova-quickshell;
          # Compiled in, so `nova` finds its UI with or without systemd. The
          # tests check builds with it too, so it reuses this build's crates.
          novaEnv.NOVA_QUICKSHELL_LAUNCHER = pkgs.lib.getExe nova-quickshell;
          # From the Rust sources alone, so a UI edit doesn't recompile it. The
          # `tests` check runs `cargo test`, so the checks that need this
          # build don't wait for it.
          bin = craneLib.buildPackage (
            common
            // {
              pname = "nova-bin";
              inherit cargoArtifacts;
              env = novaEnv;
              doCheck = false;
            }
          );
          # The binary is copied, not linked: nova finds share/nova next to
          # the file it runs from.
          nova =
            pkgs.runCommand "nova-${version}"
              {
                meta = {
                  description = "Nova desktop shell";
                  license = pkgs.lib.licenses.mit;
                  mainProgram = "nova";
                };
              }
              (
                ''
                  install -Dm755 -t $out/bin ${bin}/bin/*
                ''
                + installShare pkgs.lib "$out"
                + ''
                  install -Dm644 ${./packaging/systemd/nova.service.in} $out/lib/systemd/user/nova.service
                  substituteInPlace $out/lib/systemd/user/nova.service \
                    --replace-fail @nova@ $out/bin/nova
                  install -Dm644 ${./packaging/systemd/app-.service.d/nova.conf} $out/lib/systemd/user/app-.service.d/nova.conf
                  install -Dm644 -t $out/lib/udev/rules.d ${./features/peripherals/udev}/*.rules
                ''
              );
        };

      # The UI side on pkgs: the nova-ui program and its checks, built from
      # its own workspace (app-ui/) and lock.
      uiFor =
        pkgs:
        import ./packaging/nix/nova-ui.nix {
          inherit pkgs version advisory-db;
          craneLib = crane.mkLib pkgs;
          src = uiSrc pkgs.lib;
        };

      # The release archive install.sh fetches on other distros:
      # nova-<cpu>-linux.tar.gz with bin/nova (static, musl) and share/nova,
      # laid out like the prefix it unpacks into. Built without
      # NOVA_QUICKSHELL_LAUNCHER: there novad runs the distro's Quickshell from PATH.
      # Tests ran in the regular build; this one skips them.
      distFor =
        pkgs:
        let
          inherit (rustFor pkgs.pkgsStatic) craneLib common;
          bin = craneLib.buildPackage (
            common
            // {
              cargoArtifacts = craneLib.buildDepsOnly (common // { doCheck = false; });
              doCheck = false;
            }
          );
          name = "nova-${pkgs.stdenv.hostPlatform.parsed.cpu.name}-linux";
        in
        pkgs.runCommand "${name}-${version}" { } (
          ''
            mkdir -p tree $out
            cp -r ${bin}/bin tree/
            # Store files are read-only; an upgrade has to replace share/nova.
            chmod -R u+w tree
          ''
          + installShare pkgs.lib "tree"
          + ''
            tar -C tree --sort=name --owner=0 --group=0 --numeric-owner --mtime=@1 -c bin share \
              | gzip -n > $out/${name}.tar.gz
            (cd $out && sha256sum ${name}.tar.gz > ${name}.tar.gz.sha256)
          ''
        );

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
      # `programs.nova`: Nova, and a switch per feature that also installs
      # what it needs (packaging/nix/options.nix).
      homeManagerModules.default = import ./packaging/nix/home-manager.nix { inherit (self) packages; };
      nixosModules.default = import ./packaging/nix/nixos.nix {
        inherit (self) packages;
        novaFilesModule = nova-files.nixosModules.default;
      };

      packages = forAllSystems (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.nova;

        inherit (novaFor pkgs) nova nova-quickshell;
        # The GPUI program novad runs when features.toml says `ui = "gpui"`.
        inherit (uiFor pkgs) nova-ui;
        # What the `nova-files` feature installs.
        nova-files = nova-files.packages.${pkgs.stdenv.hostPlatform.system}.default;
        # .github/workflows/release.yml attaches this to a GitHub release.
        dist = distFor pkgs;
      });

      # Nova on the newest stable Quickshell release instead of nixpkgs': its
      # build and qmllint, for .github/workflows/newest-quickshell.yml.
      # Not checks, so `nix flake check` doesn't build a second Quickshell.
      legacyPackages = forAllSystems (
        pkgs:
        let
          newest = withNewestQuickshell pkgs;
          inherit (novaFor newest) nova;
        in
        {
          newest-quickshell = {
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
        {
          build = self.packages.${pkgs.stdenv.hostPlatform.system}.nova;
          # The UI side: nova-ui builds, and its clippy, unit tests, rustfmt
          # and cargo-deny pass (packaging/nix/nova-ui.nix).
          inherit ((uiFor pkgs).checks)
            ui-build
            ui-clippy
            ui-tests
            ui-rustfmt
            ui-cargo-deny
            ;
          tests =
            let
              inherit (rustFor pkgs) craneLib common cargoArtifacts;
            in
            craneLib.cargoTest (
              common
              // {
                inherit cargoArtifacts;
                env = (novaFor pkgs).novaEnv;
                # setpriv, for the launcher test's app that dies with its parent;
                # dbus-daemon, for the tray's and notifications' private test buses.
                nativeBuildInputs = [
                  pkgs.util-linux
                  pkgs.dbus
                ];
              }
            );
          # The release archive, so it can't break unnoticed before a tag.
          inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) dist;

          clippy =
            let
              inherit (rustFor pkgs) craneLib common cargoArtifacts;
            in
            craneLib.cargoClippy (
              common
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
              }
            );

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

          # The NixOS module with every feature on, with and without NVIDIA:
          # each manifest's `nix` packages exist in nixpkgs, and the
          # features.toml it writes reads back as the same choice, modes
          # and the UI program too.
          nix-modules =
            let
              features = import ./packaging/nix/features.nix { inherit (pkgs) lib; };
              evalWith =
                videoDrivers:
                (nixpkgs.lib.nixosSystem {
                  inherit (pkgs.stdenv.hostPlatform) system;
                  modules = [
                    self.nixosModules.default
                    {
                      boot.isContainer = true;
                      system.stateVersion = pkgs.lib.trivial.release;
                      services.xserver = { inherit videoDrivers; };
                      programs.nova = {
                        enable = true;
                        # The GPUI program as the UI, which installs it.
                        ui = "gpui";
                        # Every feature on, each with modes in its last mode
                        # (none defaults to it).
                        features = pkgs.lib.mapAttrs (
                          _: manifest:
                          {
                            enable = true;
                          }
                          // pkgs.lib.optionalAttrs (manifest ? modes) { mode = pkgs.lib.last manifest.modes; }
                        ) features.manifests;
                        # A stand-in, so the check doesn't build the Nova
                        # Files app (GPUI) just to list it.
                        novaFilesPackage = pkgs.runCommand "nova-files-stand-in" { } "mkdir $out";
                      };
                    }
                  ];
                }).config;
              report =
                config:
                let
                  selection = builtins.fromTOML config.environment.etc."xdg/nova/features.toml".text;
                in
                assert pkgs.lib.all (features.isOn selection) (builtins.attrValues features.manifests);
                assert selection.ui == "gpui";
                assert pkgs.lib.elem config.programs.nova.uiPackage config.environment.systemPackages;
                assert pkgs.lib.all (manifest: features.modeOf selection manifest == pkgs.lib.last manifest.modes) (
                  builtins.filter (manifest: manifest ? modes) (builtins.attrValues features.manifests)
                );
                # Nova Files installs through its own module, in the mode
                # picked, as the picker on niri without losing niri's other
                # portal lines.
                assert config.programs.nova-files.enable && config.programs.nova-files.mode == "full";
                assert
                  config.xdg.portal.config.niri ? "org.freedesktop.impl.portal.FileChooser"
                  && config.xdg.portal.config.niri.default == "gnome;gtk";
                pkgs.lib.concatLines (map (package: package.name) config.environment.systemPackages);
            in
            pkgs.writeText "nova-nix-modules" (
              report (evalWith [ "nvidia" ]) + report (evalWith [ "modesetting" ])
            );

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

          # The stack keeper's script (scripts/stack.sh): shellcheck, and its
          # tests on a throwaway stack with a local remote.
          stack-script =
            pkgs.runCommand "nova-stack-script"
              {
                nativeBuildInputs = [
                  pkgs.shellcheck
                  pkgs.git
                  pkgs.jq
                ];
              }
              ''
                cp ${./scripts/stack.sh} stack.sh && cp ${./scripts/test-stack.sh} test-stack.sh
                shellcheck stack.sh test-stack.sh
                patchShebangs stack.sh test-stack.sh
                ./test-stack.sh
                touch $out
              '';

          # The workspace layout (scripts/check-layout.sh): shellcheck, its
          # tests on throwaway workspaces, then the workspace itself.
          layout =
            pkgs.runCommand "nova-layout"
              {
                nativeBuildInputs = [
                  pkgs.shellcheck
                  pkgs.cargo
                  pkgs.jq
                ];
              }
              ''
                cp -r ${src}/. src && chmod -R u+w src
                # Both sides: the UI side's crates are in the layout too.
                cp -r --no-preserve=mode ${uiSrc pkgs.lib}/. src
                cp ${./scripts/check-layout.sh} check-layout.sh && cp ${./scripts/test-check-layout.sh} test-check-layout.sh
                shellcheck check-layout.sh test-check-layout.sh
                patchShebangs check-layout.sh test-check-layout.sh
                export HOME=$TMPDIR
                ./test-check-layout.sh
                ./check-layout.sh src
                touch $out
              '';

          # Test and script patterns that flaked before (scripts/lint-traps.sh).
          lint-traps = pkgs.runCommand "nova-lint-traps" { nativeBuildInputs = [ pkgs.shellcheck ]; } ''
            cp -r ${
              pkgs.lib.fileset.toSource {
                root = ./.;
                fileset = pkgs.lib.fileset.unions [
                  ./scripts
                  ./e2e
                  ./packaging
                  ./app/src
                  ./framework
                  ./features
                  ./app-ui
                  (pkgs.lib.fileset.maybeMissing ./framework-ui)
                ];
              }
            }/. src
            shellcheck src/scripts/lint-traps.sh
            bash src/scripts/lint-traps.sh src
            touch $out
          '';

          # No Rust file in a crate over 800 lines (scripts/check-file-length.sh).
          file-length = pkgs.runCommand "nova-file-length" { nativeBuildInputs = [ pkgs.shellcheck ]; } ''
            cp -r ${
              pkgs.lib.fileset.toSource {
                root = ./.;
                fileset = pkgs.lib.fileset.unions [
                  ./scripts/check-file-length.sh
                  (pkgs.lib.fileset.fileFilter (file: file.hasExt "rs") ./.)
                ];
              }
            }/. src
            shellcheck src/scripts/check-file-length.sh
            bash src/scripts/check-file-length.sh src
            touch $out
          '';

          # The blur rules `nova setup` writes for niri, against this niri.
          niri-rules = pkgs.runCommand "nova-niri-rules" { nativeBuildInputs = [ pkgs.niri ]; } ''
            niri validate -c ${./packaging/compositor/niri.kdl}
            touch $out
          '';

          # The rules `nova setup` writes for Hyprland, against this Hyprland.
          # It refuses to start as root, which a sandboxed build may look like.
          hyprland-rules = pkgs.runCommand "nova-hyprland-rules" { nativeBuildInputs = [ pkgs.hyprland ]; } ''
            export XDG_RUNTIME_DIR=$TMPDIR
            Hyprland --i-am-really-stupid --verify-config -c ${./packaging/compositor/hyprland.conf} | tee log
            grep -qx "config ok" log
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
                cd ${
                  pkgs.lib.fileset.toSource {
                    root = ./.;
                    fileset = pkgs.lib.fileset.fileFilter (file: file.hasExt "nix") ./.;
                  }
                }
                nixfmt --check $(find . -name '*.nix')
                statix check .
                deadnix --fail .
                touch $out
              '';

          # The headless e2e (e2e/run.sh: sway, two monitors, QML errors,
          # Pss) against this build. The run itself never fails, so its logs
          # and screenshot are there to look at when the check does:
          # .#checks.<system>.e2e-headless.run
          e2e-headless =
            let
              run =
                pkgs.runCommand "nova-e2e-headless-run"
                  {
                    nativeBuildInputs = [
                      pkgs.sway
                      pkgs.grim
                      pkgs.awww # so the wallpaper feature, and its carousel, loads
                      pkgs.jq
                      pkgs.dbus
                      (import ./e2e/player.nix { inherit pkgs; })
                      self.packages.${pkgs.stdenv.hostPlatform.system}.nova-quickshell
                    ];
                    FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
                  }
                  ''
                    cp -r ${e2eSrc pkgs.lib}/. . && chmod -R u+w .
                    patchShebangs e2e
                    export HOME=$TMPDIR
                    e2e/run.sh ${
                      self.packages.${pkgs.stdenv.hostPlatform.system}.nova
                    }/bin/nova >e2e.log 2>&1 || touch failed
                    mkdir -p $out
                    cp -r e2e/out/. e2e.log $out/
                    if [[ -e failed ]]; then touch $out/failed; fi
                  '';
            in
            pkgs.runCommand "nova-e2e-headless" { passthru = { inherit run; }; } ''
              cat ${run}/e2e.log
              [[ ! -e ${run}/failed ]]
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
                # Failures again at the end: Nix shows only the last lines.
                if ! qmltestrunner -input ${./ui}/tests 2>&1 | tee $TMPDIR/qml-tests.log; then
                  grep -A3 '^FAIL!' $TMPDIR/qml-tests.log
                  exit 1
                fi
                touch $out
              '';
        }
      );

      devShells = forAllSystems (pkgs: {
        # For the UI side (`nix develop .#ui`): what GPUI needs to build and
        # run, which the default shell leaves out so its Quickshell runs
        # with the session's own libraries.
        ui = (uiFor pkgs).devShell;

        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
            pkgs.quickshell
            # So dev runs show the UI as NovaUI too; the daemon finds it on PATH.
            self.packages.${pkgs.stdenv.hostPlatform.system}.nova-quickshell
            pkgs.qt6.qtdeclarative # qmllint, qmlls, qmlformat, qmltestrunner
            pkgs.qt6.qtshadertools # qsb, for scripts/shaders.sh
            pkgs.sway # headless compositor for e2e
            pkgs.grim
            pkgs.awww # e2e: the wallpaper feature (and its carousel) needs it
            pkgs.dbus # dbus-daemon, for the tray's and notifications' tests
            (import ./e2e/player.nix { inherit pkgs; }) # nova-test-player, for e2e
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
