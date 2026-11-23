# `nova-ui`: the GPUI program that draws Nova's UI, and the UI side's checks.
# It builds from its own cargo workspace and lock (app-ui/), apart from the
# daemon's, so nothing here changes when only the daemon does, and the other
# way round.
#
# Crane builds the dependencies once per profile and every step reuses
# them: GPUI is some seven hundred crates. The checks build with the `ci`
# profile (app-ui/Cargo.toml: no debug info), the package with release.
#
# `src`: the UI side's sources, laid out like the repo (the flake's uiSrc).
{
  pkgs,
  craneLib,
  src,
  version,
  advisory-db,
}:
let
  # GPUI opens these with dlopen at run time, so they go on the library path
  # of the wrapped program and of the dev shell, not only on the link line.
  runtimeLibs = with pkgs; [
    vulkan-loader
    wayland
    libxkbcommon
    libGL
  ];

  # Hashes of the git dependencies, so they are fetched as ordinary store
  # paths, not cloned at evaluation time on every run (the zed fork's clone
  # alone takes a minute). One per git source in app-ui/Cargo.lock, with
  # the same pins and hashes as Nova Files' nix/build.nix. After changing a git dependency, set its hash to
  # `pkgs.lib.fakeHash` and copy the real one from the build error.
  outputHashes = {
    "git+https://github.com/ImperaLuna/zed?rev=4cc99ed469e7cc566898de34cbf4e4c993ba2d64#4cc99ed469e7cc566898de34cbf4e4c993ba2d64" =
      "sha256-EIRsVvKahgBN8k63ui/GTQuMtjRmA3qV1OJLw2BApRs=";
    "git+https://github.com/gfx-rs/wgpu?rev=cddb2f5d80dc0009b2ef6767635140cacf90a007#cddb2f5d80dc0009b2ef6767635140cacf90a007" =
      "sha256-OqPeK1cr3OSZgIl+F3GMuXWz31PxFoOO0u4INN9GmgM=";
    "git+https://github.com/zed-industries/font-kit?rev=94b0f28166665e8fd2f53ff6d268a14955c82269#94b0f28166665e8fd2f53ff6d268a14955c82269" =
      "sha256-KXygi0olNQi5yM8eaJVykNDtbPMDjT+cWPBF8UrtXR4=";
    "git+https://github.com/zed-industries/oo7?rev=731c1215edb6905876c6137ab300428c7b8d7c13#731c1215edb6905876c6137ab300428c7b8d7c13" =
      "sha256-QXblWsnq+graWOSW0iA8pUzpwizDLi/kOSWDdSQVZ/0=";
    "git+https://github.com/zed-industries/scap?rev=b5c86ec58dd4b6b7732bf6ac8be03d6b1f190a82#b5c86ec58dd4b6b7732bf6ac8be03d6b1f190a82" =
      "sha256-WVcjceVESUhPKL9z/1y+l/vmtHRJS4EecaMOaVN686o=";
    "git+https://github.com/zed-industries/wasm_thread?rev=0cf96c7708dfb97ccf3da50347e25edcf75d6937#0cf96c7708dfb97ccf3da50347e25edcf75d6937" =
      "sha256-+lRLCIk0S6Y5ORYjDKsYYHia2FtoSoh+rWkQh7mnPBE=";
  };

  # What building GPUI needs.
  buildTools = with pkgs; [
    pkg-config
    cmake
    rustPlatform.bindgenHook
  ];
  buildLibs = with pkgs; [
    fontconfig
    freetype
    libxkbcommon
    wayland
    zstd
  ];

  common = {
    pname = "nova-ui";
    inherit version src outputHashes;
    # The workspace is in a folder of the sources, not at their root.
    cargoToml = ../../app-ui/Cargo.toml;
    cargoLock = ../../app-ui/Cargo.lock;
    postUnpack = ''
      cd $sourceRoot/app-ui
      sourceRoot="."
    '';
    strictDeps = true;
    nativeBuildInputs = buildTools ++ [ pkgs.makeWrapper ];
    buildInputs = buildLibs;
  };

  ci = common // {
    CARGO_PROFILE = "ci";
  };
  ciArtifacts = craneLib.buildDepsOnly ci;

  package =
    args: cargoArtifacts:
    craneLib.buildPackage (
      args
      // {
        inherit cargoArtifacts;
        doCheck = false;
        postFixup = ''
          wrapProgram $out/bin/nova-ui \
            --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtimeLibs}
        '';
        meta = {
          description = "Nova's UI, drawn with GPUI";
          license = pkgs.lib.licenses.mit;
          mainProgram = "nova-ui";
          platforms = pkgs.lib.platforms.linux;
        };
      }
    );

  # The same folder name cargo-deny gives RustSec's database (flake.nix's
  # cargo-deny check says more).
  advisoryDbFolder = "advisory-db-3157b0e258782691";
in
{
  nova-ui = package common (craneLib.buildDepsOnly common);

  checks = {
    # The program builds and installs, without a release build.
    ui-build = package ci ciArtifacts;

    ui-clippy = craneLib.cargoClippy (
      ci
      // {
        cargoArtifacts = ciArtifacts;
        cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
      }
    );

    # The UI side's unit tests (pure logic only: it has no UI test suite).
    ui-tests = craneLib.cargoTest (
      ci
      // {
        cargoArtifacts = ciArtifacts;
        cargoTestExtraArgs = "--workspace";
      }
    );

    ui-rustfmt = craneLib.cargoFmt {
      inherit (common)
        pname
        version
        src
        cargoToml
        cargoLock
        postUnpack
        ;
    };

    # Advisories, licenses, banned crates and sources (app-ui/deny.toml),
    # against the advisory database pinned in flake.lock.
    ui-cargo-deny = craneLib.mkCargoDerivation (
      common
      // {
        pnameSuffix = "-deny";
        cargoArtifacts = null;
        doInstallCargoArtifacts = false;
        nativeBuildInputs = common.nativeBuildInputs ++ [
          pkgs.cargo-deny
          pkgs.git
        ];
        buildPhaseCargoCommand = ''
          db=$CARGO_HOME/advisory-dbs/${advisoryDbFolder}
          mkdir -p $db && cp -r ${advisory-db}/. $db && chmod -R u+w $db
          export GIT_AUTHOR_DATE=@${toString advisory-db.lastModified} GIT_COMMITTER_DATE=@${toString advisory-db.lastModified}
          git -C $db init -q && git -C $db add -A
          git -C $db -c user.name=nix -c user.email=nix@localhost commit -q -m advisory-db
          cargo deny --offline check
        '';
      }
    );
  };

  # `nix develop .#ui`: cargo and what GPUI needs to build and to run.
  devShell = pkgs.mkShell {
    nativeBuildInputs = buildTools;
    buildInputs = buildLibs;
    packages = with pkgs; [
      cargo
      rustc
      clippy
      rustfmt
      rust-analyzer
      cargo-deny
      just
      jq
      git
    ];
    LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
  };
}
