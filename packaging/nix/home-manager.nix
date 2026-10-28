# Nova's home-manager module (homeManagerModules.default): Nova, the
# packages of the features that are on, its systemd user unit started with
# graphical-session.target, and the features.toml novad reads.
#
#   programs.nova.enable = true;
#   programs.nova.features.wallpaper-engine.enable = true;
#
# `packages`: the flake's packages, by system.
{ packages }:
{
  config,
  lib,
  pkgs,
  osConfig ? null,
  ...
}:
let
  features = import ./features.nix { inherit lib; };
  cfg = config.programs.nova;
  on = features.onIds cfg.features;
  unit = "${cfg.package}/lib/systemd/user/nova.service";
in
{
  imports = [ (import ./options.nix { inherit packages; }) ];

  config = lib.mkIf cfg.enable {
    # Inside a NixOS configuration, the GPU it drives.
    programs.nova.gpu = lib.mkIf (osConfig != null) (lib.mkDefault (features.gpuOf osConfig));

    home.packages = [ cfg.package ] ++ features.packagesFor pkgs cfg.gpu on;

    xdg.configFile = {
      "nova/features.toml".text = features.selectionFile on;
      # The package's own unit, linked where systemd looks, and wanted by
      # graphical-session.target (which niri, and Hyprland through uwsm,
      # start).
      "systemd/user/nova.service".source = unit;
      "systemd/user/graphical-session.target.wants/nova.service".source = unit;
      # Orders login apps (XDG autostart) after Nova, so they find its tray.
      "systemd/user/app-.service.d/nova.conf".source =
        "${cfg.package}/lib/systemd/user/app-.service.d/nova.conf";
    };

    warnings =
      lib.optional
        (
          features.usesGpuScreenRecorder cfg.gpu on
          && !(osConfig.programs.gpu-screen-recorder.enable or false)
        )
        "Nova's screen recording uses gpu-screen-recorder, whose capture helper needs `programs.gpu-screen-recorder.enable = true;` in your NixOS configuration (or Nova's NixOS module).";
  };
}
