# Nova's home-manager module (homeManagerModules.default): Nova, the
# packages of the features that are on, its systemd user unit started with
# graphical-session.target, the features.toml novad reads, the Nova Files
# app while the `nova-files` feature is on, and Nova's compositor rules
# (blur) in ~/.config/nova: include
# niri.kdl from niri's config; home-manager's own Hyprland config sources
# hyprland.conf by itself.
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
  novaFilesOn = lib.elem "nova-files" on;
  # Nova Files' modes come from its NixOS module (Nova's NixOS module passes
  # the feature's mode on); home-manager can only install the whole app.
  novaFilesBySystem = osConfig.programs.nova-files.enable or false;
  unit = "${cfg.package}/lib/systemd/user/nova.service";
in
{
  imports = [ (import ./options.nix { inherit packages; }) ];

  config = lib.mkIf cfg.enable {
    # Inside a NixOS configuration, the GPU it drives.
    programs.nova.gpu = lib.mkIf (osConfig != null) (lib.mkDefault (features.gpuOf osConfig));

    home.packages = [
      cfg.package
    ]
    ++ features.packagesFor pkgs cfg.gpu on
    # The Nova Files app, unless the system installs it; the session bus
    # finds its D-Bus service in the profile's share/dbus-1/services.
    ++ lib.optional (novaFilesOn && !novaFilesBySystem) cfg.novaFilesPackage;
    xdg = {
      configFile = {
        "nova/features.toml".text = features.selectionFile cfg.features;
        # The package's own unit, linked where systemd looks, and wanted by
        # graphical-session.target (which niri, and Hyprland through uwsm,
        # start).
        "systemd/user/nova.service".source = unit;
        "systemd/user/graphical-session.target.wants/nova.service".source = unit;
        # Orders login apps (XDG autostart) after Nova, so they find its tray.
        "systemd/user/app-.service.d/nova.conf".source =
          "${cfg.package}/lib/systemd/user/app-.service.d/nova.conf";
        # The same files `nova setup` writes on other distros.
        "nova/niri.kdl".source = ../compositor/niri.kdl;
        "nova/hyprland.conf".source = ../compositor/hyprland.conf;
      };
    };
    # Sourced first, so the user's own rules win.
    wayland.windowManager.hyprland.settings.source =
      lib.mkIf config.wayland.windowManager.hyprland.enable
        [
          "${config.xdg.configHome}/nova/hyprland.conf"
        ];

    warnings =
      lib.optional
        (
          features.usesGpuScreenRecorder cfg.gpu on
          && !(osConfig.programs.gpu-screen-recorder.enable or false)
        )
        "Nova's screen recording uses gpu-screen-recorder, whose capture helper needs `programs.gpu-screen-recorder.enable = true;` in your NixOS configuration (or Nova's NixOS module)."
      ++
        lib.optional (novaFilesOn && !novaFilesBySystem && cfg.features.nova-files.mode != "basic")
          "Nova Files' `${cfg.features.nova-files.mode}` mode needs Nova's NixOS module; home-manager installs the whole app, as in the `basic` mode, without making it the folder default or file picker.";
  };
}
