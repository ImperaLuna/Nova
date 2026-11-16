# `nova-ui`: runs Quickshell as NovaUI (novad's UI_PROCESS_NAME), so ps and
# btop show the UI as Nova's in both the command line and the process name.
#
# The kernel names a process after the file it executes, and nixpkgs runs
# Quickshell's real binary as `.quickshell-wrapped`. So this execs a symlink
# named NovaUI to that binary, from a wrapper that sets the same Qt paths
# nixpkgs' own wrapper does. --inherit-argv0 keeps the `NovaUI` argv[0]
# that novad passes.
#
# Quickshell is built here with two patches, so it compiles from source
# instead of coming from the binary cache (a few minutes, once per nixpkgs
# update). Both fix crashes while glass keeps a screen copy:
# - quickshell-wlr-output.patch: its wlr-screencopy code binds each monitor
#   a second time as a QtWayland::wl_output, which Qt mistakes for a screen,
#   so a Nova window leaving a monitor on niri crashed the UI (IMP-262).
# - quickshell-null-screen.patch: upstream's fix from after 0.3.2. A copy
#   of a monitor that dropped off read its gone screen, which crashed the UI
#   each time screens woke from DPMS off on Hyprland (IMP-360).
# Drop each patch, and this override with the last, once nixpkgs ships a
# Quickshell with it fixed upstream; a patch stops applying when its file
# changes, so the "Newest Quickshell" check flags it.
{
  stdenv,
  qt6,
  makeBinaryWrapper,
  quickshell,
}:
let
  patched = quickshell.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [
      ./quickshell-wlr-output.patch
      ./quickshell-null-screen.patch
    ];
  });
in
stdenv.mkDerivation {
  pname = "nova-ui";
  inherit (patched) version;
  dontUnpack = true;
  nativeBuildInputs = [
    qt6.wrapQtAppsHook
    makeBinaryWrapper
  ];
  # Quickshell's inputs, so wrapQtAppsHook collects the same plugin and QML
  # paths into qtWrapperArgs; Quickshell itself for its own QML modules.
  buildInputs = patched.buildInputs ++ [ patched ];
  dontWrapQtApps = true;
  installPhase = ''
    runHook preInstall
    mkdir -p $out/libexec/nova $out/bin
    ln -s ${patched}/bin/.quickshell-wrapped $out/libexec/nova/NovaUI
    makeBinaryWrapper $out/libexec/nova/NovaUI $out/bin/nova-ui --inherit-argv0 \
      --prefix XDG_DATA_DIRS : ${patched}/share "''${qtWrapperArgs[@]}"
    runHook postInstall
  '';
  meta = {
    description = "Quickshell, run under the name NovaUI";
    inherit (patched.meta) license platforms;
    mainProgram = "nova-ui";
  };
}
