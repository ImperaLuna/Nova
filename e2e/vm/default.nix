# NixOS VM tests: Nova's lifecycle under systemd on real niri and Hyprland.
# Each VM logs a user in on tty1, whose login shell starts the compositor,
# and installs Nova the way a NixOS user would (the package and its user
# unit). Then lifecycle.py drives it with the `nova` command.
#
# They need KVM, which the self-hosted runner has. Run one with
#   nix build .#checks.x86_64-linux.e2e-niri -L
# or interactively with `.driverInteractive`.
{ pkgs, nova }:
let
  user = "alice";
  uid = 1000;

  mkTest =
    {
      compositor,
      config,
      session,
      quitCompositor,
    }:
    pkgs.testers.runNixOSTest {
      name = "nova-${compositor}";
      nodes.machine = {
        imports = [ config ];
        users.users.${user} = {
          isNormalUser = true;
          inherit uid;
        };
        services.getty.autologinUser = user;
        # Not `exec`: when a test quits the compositor, the shell stays and
        # getty doesn't log in again and restart it.
        programs.bash.loginShellInit = ''
          if [ "$(tty)" = /dev/tty1 ]; then ${session}; fi
        '';
        environment.systemPackages = [ nova ];
        systemd.packages = [ nova ];
        # NixOS ignores [Install] in packaged units; this is the unit's
        # WantedBy=.
        systemd.user.services.nova.wantedBy = [ "graphical-session.target" ];
        hardware.graphics.enable = true;
        virtualisation = {
          memorySize = 3072;
          cores = 2;
          # A virtio GPU gives the compositor a DRM device; Mesa renders on
          # the CPU (llvmpipe).
          qemu.options = [
            "-vga none"
            "-device virtio-gpu-pci"
          ];
        };
      };
      testScript = ''
        USER = ${builtins.toJSON user}
        RUNTIME_DIR = "/run/user/${toString uid}"
        QUIT_COMPOSITOR = ${builtins.toJSON quitCompositor}
      ''
      + builtins.readFile ./lifecycle.py;
    };

  hyprlandConfig = pkgs.writeText "hyprland.conf" ''
    # How Nova starts on Hyprland without uwsm (see nova.service).
    exec-once = nova
  '';
in
{
  # niri-session runs niri as a systemd unit and activates
  # graphical-session.target, which starts nova.service.
  e2e-niri = mkTest {
    compositor = "niri";
    config.programs.niri.enable = true;
    session = "niri-session";
    quitCompositor = "NIRI_SOCKET=$(echo $XDG_RUNTIME_DIR/niri.*.sock) niri msg action quit --skip-confirmation";
  };

  # Plain Hyprland, no uwsm: nothing activates graphical-session.target, so
  # `exec-once = nova` hands the session to systemd and starts the unit.
  e2e-hyprland = mkTest {
    compositor = "hyprland";
    config.programs.hyprland.enable = true;
    session = "Hyprland --config ${hyprlandConfig}";
    quitCompositor = "HYPRLAND_INSTANCE_SIGNATURE=$(ls $XDG_RUNTIME_DIR/hypr | head -n1) hyprctl dispatch exit";
  };
}
