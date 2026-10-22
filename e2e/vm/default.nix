# NixOS VM tests on real niri and Hyprland. Each VM logs a user in on tty1,
# whose login shell starts the compositor, and installs Nova the way a NixOS
# user would (the package and its user unit). lifecycle.py then drives Nova
# with the `nova` command and screenshots each UI state, and compare.sh
# checks the screenshots against e2e/golden/<compositor>.
#
# They need KVM, which the self-hosted runner has (through the Nix daemon,
# so run them as Nix builds). Per compositor:
#   checks.<system>.e2e-<compositor>          the check: lifecycle + screenshots
#   checks.<system>.e2e-<compositor>.vm       the VM test and its screenshots
#   checks.<system>.e2e-<compositor>.compare  report.txt, screenshots/, diff/
{ pkgs, nova }:
let
  user = "alice";
  uid = 1000;

  mkTest =
    {
      compositor,
      config,
      session,
      # The compositor's IPC command, with what it needs to find the
      # compositor from a user shell.
      ipc,
      quitCompositor,
    }:
    let
      vm = pkgs.testers.runNixOSTest {
        name = "nova-${compositor}";
        nodes.machine = {
          imports = [ config ];
          users.users.${user} = {
            isNormalUser = true;
            inherit uid;
            extraGroups = [ "seat" ];
          };
          services = {
            getty.autologinUser = user;
            # The compositors get the GPU and input devices from seatd.
            # Through logind, niri (a user unit) at times got ENODEV for the
            # GPU while logind was still working through the boot's device
            # events.
            seatd.enable = true;
          };
          environment = {
            variables.LIBSEAT_BACKEND = "seatd";
            # Qt otherwise picks subpixel or grayscale text from the monitor's
            # subpixel info, depending on whether that has arrived yet, so
            # screenshots of text would differ from run to run. Set through
            # PAM, so the systemd user manager (and nova.service) has it.
            sessionVariables.QT_SUBPIXEL_AA_TYPE = "RGB";
            # grim takes the screenshots.
            systemPackages = [
              nova
              pkgs.grim
            ];
          };
          # Only in the interactive shell getty starts: niri-session runs
          # itself again in a non-interactive login shell. Not `exec`: when a
          # test quits the compositor, the shell stays and getty doesn't log
          # in again and restart it.
          programs.bash.loginShellInit = ''
            if [[ $- == *i* && $(tty) == /dev/tty1 ]]; then ${session}; fi
          '';
          systemd = {
            packages = [ nova ];
            # NixOS ignores [Install] in packaged units; this is the unit's
            # WantedBy=.
            user.services.nova.wantedBy = [ "graphical-session.target" ];
            # Under emulation the login can come before udev has set up the
            # GPU, and the compositor then finds no display.
            services."getty@tty1" = {
              wants = [ "systemd-udev-settle.service" ];
              after = [ "systemd-udev-settle.service" ];
            };
          };
          hardware.graphics.enable = true;
          virtualisation = {
            memorySize = 3072;
            cores = 2;
          };
        };
        testScript = ''
          USER = ${builtins.toJSON user}
          UID = ${toString uid}
          QUIT_COMPOSITOR = ${builtins.toJSON "${ipc} ${quitCompositor}"}
        ''
        + builtins.readFile ./lifecycle.py;
      };

      golden = ../golden + "/${compositor}";

      # Never fails, so its diff images are there to look at when the check
      # does.
      compare =
        pkgs.runCommand "nova-${compositor}-screenshots" { nativeBuildInputs = [ pkgs.odiff ]; }
          ''
            mkdir -p $out/screenshots
            cp ${vm}/*.png $out/screenshots/
            bash ${./compare.sh} ${vm} ${
              if builtins.pathExists golden then golden else "/nonexistent"
            } $out || touch $out/failed
          '';
    in
    pkgs.runCommand "nova-${compositor}-e2e"
      {
        passthru = { inherit vm compare; };
      }
      ''
        cat ${compare}/report.txt
        if [[ -e ${compare}/failed ]]; then
          echo "Screenshots differ from e2e/golden/${compositor}. Diffs: nix build .#checks.${pkgs.stdenv.hostPlatform.system}.e2e-${compositor}.compare"
          exit 1
        fi
        touch $out
      '';

  # The VM's monitor. There's no GPU in CI, so Mesa renders on the CPU
  # (llvmpipe), and each compositor takes that only one way. The driver
  # loads in the initrd, so it's there before the compositor starts.
  #
  # QEMU's standard VGA (bochs-drm): display-only, no render node.
  standardVga = {
    boot.initrd.kernelModules = [ "bochs" ];
    virtualisation.qemu.options = [ "-vga std" ];
  };
  # A virtio GPU without 3D: it has a render node, but Mesa has to be told to
  # use its software renderer for it.
  virtioGpu = {
    boot.initrd.kernelModules = [ "virtio_gpu" ];
    virtualisation.qemu.options = [
      "-vga none"
      "-device virtio-gpu-pci"
    ];
    environment.variables = {
      GBM_ALWAYS_SOFTWARE = "1";
      LIBGL_ALWAYS_SOFTWARE = "1";
    };
  };

  hyprlandConfig = pkgs.writeText "hyprland.conf" ''
    # How Nova starts on Hyprland without uwsm (see nova.service).
    exec-once = nova
    # A plain background and no cursor or random splash text, so screenshots
    # are the same every run.
    misc {
      disable_hyprland_logo = true
      disable_splash_rendering = true
      force_default_wallpaper = 0
    }
    cursor {
      invisible = true
    }
  '';
in
{
  # niri-session runs niri as a systemd unit and activates
  # graphical-session.target, which starts nova.service.
  e2e-niri = mkTest {
    compositor = "niri";
    config = {
      # niri renders with the render node when there is one, which the
      # software renderer doesn't support.
      imports = [ standardVga ];
      programs.niri = {
        enable = true;
        # niri skips software renderers (src/backend/tty.rs, "software EGL
        # renderers are skipped"), and a VM has nothing else, so niri would
        # run without a monitor. This build drops that check; remove it once
        # niri has a switch for it. Its tests already ran in nixpkgs' build.
        package = pkgs.niri.overrideAttrs (old: {
          postPatch = (old.postPatch or "") + ''
            substituteInPlace src/backend/tty.rs \
              --replace-fail '!egl_device.is_software(),' 'true,'
          '';
          doCheck = false;
        });
      };
      # niri's own defaults, without the key help it shows at startup or
      # the cursor, so screenshots show only Nova.
      environment.etc."niri/config.kdl".text = ''
        hotkey-overlay {
          skip-at-startup
        }
        cursor {
          hide-after-inactive-ms 1000
        }
      '';
    };
    session = "niri-session";
    ipc = "NIRI_SOCKET=$(echo $XDG_RUNTIME_DIR/niri.*.sock) niri msg";
    quitCompositor = "action quit --skip-confirmation";
  };

  # Plain Hyprland, no uwsm: nothing activates graphical-session.target, so
  # `exec-once = nova` hands the session to systemd and starts the unit.
  e2e-hyprland = mkTest {
    compositor = "hyprland";
    config = {
      # Hyprland needs a render node: on the standard VGA it aborts.
      imports = [ virtioGpu ];
      programs.hyprland.enable = true;
    };
    session = "Hyprland --config ${hyprlandConfig}";
    ipc = "HYPRLAND_INSTANCE_SIGNATURE=$(ls $XDG_RUNTIME_DIR/hypr | head -n1) hyprctl";
    quitCompositor = "dispatch exit";
  };
}
