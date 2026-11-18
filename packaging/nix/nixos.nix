# Nova's NixOS module (nixosModules.default): Nova, the packages of the
# features that are on, its systemd user unit started with
# graphical-session.target, a system-wide features.toml in /etc/xdg/nova,
# which novad reads when the user has none of their own, and the Nova Files
# app with its D-Bus service while the `nova-files` feature is on.
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
  novaFilesOn = lib.elem "nova-files" on;
in
{
  imports = [ (import ./options.nix { inherit packages; }) ];

  config = lib.mkIf cfg.enable {
    programs.nova.gpu = lib.mkDefault (features.gpuOf config);

    environment = {
      systemPackages = [
        cfg.package
      ]
      ++ features.packagesFor pkgs cfg.gpu on
      ++ lib.optional novaFilesOn cfg.novaFilesPackage;
      etc."xdg/nova/features.toml".text = features.selectionFile cfg.features;
    };
    # The Nova Files app's D-Bus service, so the launcher's first search
    # starts it.
    services.dbus.packages = lib.mkIf novaFilesOn [ cfg.novaFilesPackage ];

    systemd.packages = [ cfg.package ];
    # NixOS ignores [Install] in packaged units; this is the unit's WantedBy=.
    systemd.user.services.nova.wantedBy = [ "graphical-session.target" ];

    # gpu-screen-recorder's capture helper, for screen recording on NVIDIA.
    programs.gpu-screen-recorder.enable = lib.mkIf (features.usesGpuScreenRecorder cfg.gpu on) (
      lib.mkDefault true
    );

    services = {
      # Device batteries: novad reads upowerd on the system bus,
      # HeadsetControl needs its udev rules to open headsets without root,
      # and Nova's own rules let novad open Logitech's and Keychron's hidraw
      # nodes.
      upower.enable = lib.mkIf (lib.elem "peripherals" on) (lib.mkDefault true);
      udev.packages = lib.mkIf (lib.elem "peripherals" on) [
        pkgs.headsetcontrol
        cfg.package
      ];

      # Bluetooth outputs stay awake through silence. WirePlumber suspends
      # an idle output after 5 s, which releases the A2DP stream: headsets
      # beep as if they dropped, and the next sound starts late.
      #
      # A headset's mic only works in its call profile, so WirePlumber
      # switches to it while something records and back to music quality
      # when recording stops. Its 2 to 4 s wait before switching back made
      # every dictation pay a switch and a ping; Nova waits the most it
      # allows, a minute, so back-to-back dictation stays in call mode.
      # WirePlumber reads this from 0.5.19 on.
      pipewire.wireplumber.extraConfig."51-nova-bluetooth" = lib.mkIf (lib.elem "bluetooth" on) {
        "wireplumber.settings"."bluetooth.autoswitch-restore-timeout" = 60000;
        "monitor.bluez.rules" = [
          {
            matches = [ { "node.name" = "~bluez_output.*"; } ];
            actions.update-props."session.suspend-timeout-seconds" = 0;
          }
        ];
      };
    };

    # Bluetooth: novad talks to bluetoothd on the system bus.
    hardware.bluetooth.enable = lib.mkIf (lib.elem "bluetooth" on) (lib.mkDefault true);
  };
}
