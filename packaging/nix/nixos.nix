# Nova's NixOS module (nixosModules.default): Nova, the packages of the
# features that are on, its systemd user unit started with
# graphical-session.target, a system-wide features.toml in /etc/xdg/nova,
# which novad reads when the user has none of their own, and Nova Files
# while the `nova-files` feature is on.
#
#   programs.nova.enable = true;
#   programs.nova.features.wallpaper-engine.enable = true;
#
# Nova Files installs through its own module (`programs.nova-files`), which
# this one imports, in the feature's mode:
#
#   programs.nova.features.nova-files = { enable = true; mode = "full"; };
#
# So don't import Nova Files' module again. Its file picker goes in niri's
# and Hyprland's portals.conf, which replace the ones they ship, so their
# other lines are set here too.
#
# `packages`: the flake's packages, by system. `novaFilesModule`: Nova
# Files' NixOS module.
{ packages, novaFilesModule }:
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
  # The desktops Nova runs on, as `XDG_CURRENT_DESKTOP` names them in lower
  # case, and the lines of the portals.conf each ships, which a picker
  # set there replaces.
  desktopPortals = {
    niri = {
      default = [
        "gnome"
        "gtk"
      ];
      "org.freedesktop.impl.portal.Access" = [ "gtk" ];
      "org.freedesktop.impl.portal.Notification" = [ "gtk" ];
      "org.freedesktop.impl.portal.Secret" = [ "gnome-keyring" ];
    };
    hyprland.default = [
      "hyprland"
      "gtk"
    ];
  };
in
{
  imports = [
    (import ./options.nix { inherit packages; })
    novaFilesModule
  ];

  config = lib.mkIf cfg.enable {
    programs = {
      nova.gpu = lib.mkDefault (features.gpuOf config);

      nova-files = lib.mkIf novaFilesOn {
        enable = true;
        inherit (cfg.features.nova-files) mode;
        package = lib.mkDefault cfg.novaFilesPackage;
        filePicker.desktops = lib.mkDefault (lib.attrNames desktopPortals);
      };

      # gpu-screen-recorder's capture helper, for screen recording on NVIDIA.
      gpu-screen-recorder.enable = lib.mkIf (features.usesGpuScreenRecorder cfg.gpu on) (
        lib.mkDefault true
      );
    };

    environment = {
      systemPackages = [
        cfg.package
      ]
      # novad finds `nova-ui` on PATH.
      ++ lib.optional (cfg.ui == "gpui") cfg.uiPackage
      ++ features.packagesFor pkgs cfg.gpu on;
      etc."xdg/nova/features.toml".text = features.selectionFile cfg.ui cfg.features;
    };

    xdg.portal.config = lib.mkIf (novaFilesOn && config.programs.nova-files.filePicker.enable) (
      lib.mapAttrs (_: lib.mapAttrs (_: lib.mkDefault)) (
        lib.filterAttrs (
          desktop: _: lib.elem desktop config.programs.nova-files.filePicker.desktops
        ) desktopPortals
      )
    );

    systemd.packages = [ cfg.package ];
    # NixOS ignores [Install] in packaged units; this is the unit's WantedBy=.
    systemd.user.services.nova.wantedBy = [ "graphical-session.target" ];

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
