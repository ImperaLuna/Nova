# A media player for the e2e tests' now-playing pill: mpv with its MPRIS
# script, and `nova-test-player`, which plays a quiet tone forever under a
# fixed title, with no sound card or window needed. mpv's desktop entry and
# icon come along, so novad finds it as an app.
{ pkgs }:
let
  mpv = pkgs.mpv.override { scripts = [ pkgs.mpvScripts.mpris ]; };
  player = pkgs.writeShellScriptBin "nova-test-player" ''
    exec ${mpv}/bin/mpv --no-config --idle=no --ao=null --vo=null \
      --force-media-title='Aruarian Dance' --loop-file=inf \
      'av://lavfi:sine=frequency=440' "$@"
  '';
in
pkgs.symlinkJoin {
  name = "nova-test-player";
  paths = [
    player
    mpv
  ];
}
