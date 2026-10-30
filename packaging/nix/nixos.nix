# Nova's NixOS module (nixosModules.default): Nova, the packages of the
# features that are on, its systemd user unit started with
# graphical-session.target, and a system-wide features.toml in /etc/xdg/nova,
# which novad reads when the user has none of their own.
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
  ...
}:
let
  features = import ./features.nix { inherit lib; };
  cfg = config.programs.nova;
  on = features.onIds cfg.features;
in
{
  imports = [ (import ./options.nix { inherit packages; }) ];

  config = lib.mkIf cfg.enable {
    programs.nova.gpu = lib.mkDefault (features.gpuOf config);

    environment.systemPackages = [ cfg.package ] ++ features.packagesFor pkgs cfg.gpu on;
    environment.etc."xdg/nova/features.toml".text = features.selectionFile on;

    systemd.packages = [ cfg.package ];
    # NixOS ignores [Install] in packaged units; this is the unit's WantedBy=.
    systemd.user.services.nova.wantedBy = [ "graphical-session.target" ];

    # gpu-screen-recorder's capture helper, for screen recording on NVIDIA.
    programs.gpu-screen-recorder.enable = lib.mkIf (features.usesGpuScreenRecorder cfg.gpu on) (
      lib.mkDefault true
    );

    # Device batteries: novad reads upowerd on the system bus,
    # HeadsetControl needs its udev rules to open headsets without root, and
    # Nova's own rules let novad open Logitech's and Keychron's hidraw nodes.
    services.upower.enable = lib.mkIf (lib.elem "peripherals" on) (lib.mkDefault true);
    services.udev.packages = lib.mkIf (lib.elem "peripherals" on) [
      pkgs.headsetcontrol
      cfg.package
    ];

    # Bluetooth: novad talks to bluetoothd on the system bus.
    hardware.bluetooth.enable = lib.mkIf (lib.elem "bluetooth" on) (lib.mkDefault true);
  };
}
