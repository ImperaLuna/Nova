# `nova-ui`: Quickshell under the name nova-ui, so ps and btop show the UI as
# Nova's in both the command line and the process name.
#
# The kernel names a process after the file it executes, and nixpkgs runs
# Quickshell's real binary as `.quickshell-wrapped`. So this exec's a symlink
# named nova-ui to that binary, from a wrapper that sets the same Qt paths
# nixpkgs' own wrapper does. --inherit-argv0 keeps the `nova-ui` argv[0]
# that novad passes.
{
  stdenv,
  qt6,
  makeBinaryWrapper,
  quickshell,
}:
stdenv.mkDerivation {
  pname = "nova-ui";
  inherit (quickshell) version;
  dontUnpack = true;
  nativeBuildInputs = [
    qt6.wrapQtAppsHook
    makeBinaryWrapper
  ];
  # Quickshell's inputs, so wrapQtAppsHook collects the same plugin and QML
  # paths into qtWrapperArgs; Quickshell itself for its own QML modules.
  buildInputs = quickshell.buildInputs ++ [ quickshell ];
  dontWrapQtApps = true;
  installPhase = ''
    runHook preInstall
    mkdir -p $out/libexec/nova $out/bin
    ln -s ${quickshell}/bin/.quickshell-wrapped $out/libexec/nova/nova-ui
    makeBinaryWrapper $out/libexec/nova/nova-ui $out/bin/nova-ui --inherit-argv0 \
      --prefix XDG_DATA_DIRS : ${quickshell}/share "''${qtWrapperArgs[@]}"
    runHook postInstall
  '';
  meta = {
    description = "Quickshell, run under the name nova-ui";
    inherit (quickshell.meta) license platforms;
    mainProgram = "nova-ui";
  };
}
