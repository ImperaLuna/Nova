# Nova's home-manager module (homeManagerModules.default): Nova, the
# packages of the features that are on, its systemd user unit started with
# graphical-session.target, the features.toml novad reads, the Nova Files
# app while the `nova-files` feature is on, and Nova's compositor rules
# (blur, and Hyprland's lock restore) in ~/.config/nova: include niri.kdl
# from niri's config; home-manager's own Hyprland config sources
# hyprland.conf by itself, or requires hyprland.lua when it writes a Lua
# config.
#
#   programs.nova.enable = true;
#   programs.nova.features.wallpaper-engine.enable = true;
#
# `packages`: the flake's packages, by system.
{ packages }:
{
  config,
  options,
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
  hyprland = config.wayland.windowManager.hyprland;
  # A home-manager with `configType` may write hyprland.lua instead of
  # hyprland.conf (Hyprland 0.57 reads only that).
  hyprlandLua = (options.wayland.windowManager.hyprland ? configType) && hyprland.configType == "lua";
in
{
  imports = [ (import ./options.nix { inherit packages; }) ];

  config = lib.mkIf cfg.enable {
    programs.nova = {
      # Inside a NixOS configuration, the GPU it drives.
      gpu = lib.mkIf (osConfig != null) (lib.mkDefault (features.gpuOf osConfig));
      # The UI program Nova's NixOS module picks, when it's in use too: this
      # module's features.toml is read before the system's.
      ui = lib.mkIf (osConfig.programs.nova.enable or false) (lib.mkDefault osConfig.programs.nova.ui);
      # Nova Files installed by the system runs in the system's mode, which
      # novad has to know to route folders.
      features.nova-files.mode = lib.mkIf novaFilesBySystem (
        lib.mkDefault osConfig.programs.nova-files.mode
      );
    };

    home.packages = [
      cfg.package
    ]
    # novad finds `nova-ui` on PATH.
    ++ lib.optional (cfg.ui == "gpui") cfg.uiPackage
    ++ features.packagesFor pkgs cfg.gpu on
    # The Nova Files app, unless the system installs it; the session bus
    # finds its D-Bus service in the profile's share/dbus-1/services.
    ++ lib.optional (novaFilesOn && !novaFilesBySystem) cfg.novaFilesPackage;
    xdg = {
      configFile = {
        "nova/features.toml".text = features.selectionFile cfg.ui cfg.features;
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
        "nova/hyprland.lua".source = ../compositor/hyprland.lua;
      };
    };
    wayland.windowManager.hyprland = {
      # Sourced first, so the user's own rules win.
      settings.source = lib.mkIf (hyprland.enable && !hyprlandLua) [
        "${config.xdg.configHome}/nova/hyprland.conf"
      ];
    }
    # Written as ~/.config/hypr/nova-rules.lua and required from the
    # hyprland.lua home-manager writes. Only a home-manager that has the
    # option may be given it.
    // lib.optionalAttrs (options.wayland.windowManager.hyprland ? extraLuaFiles) {
      extraLuaFiles = lib.mkIf (hyprland.enable && hyprlandLua) {
        nova-rules = ../compositor/hyprland.lua;
      };
    };

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
