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
#   checks.<system>.e2e-<compositor>.record   recorded IPC data (record.py)
{ pkgs, nova }:
let
  user = "alice";
  uid = 1000;

  mkTest =
    {
      compositor,
      config,
      session,
      # The variable a user shell needs to reach the compositor's IPC, as
      # `NAME=value`, and its IPC command.
      ipcEnv,
      ipc,
      quitCompositor,
      # For the recording (record.py): the command that streams the
      # compositor's events, its state queries by name, and its commands for
      # the steps that differ between compositors.
      eventStream,
      queries,
      steps,
      # Prints the compositor's recent log, for when a step fails.
      compositorLog,
    }:
    let
      # The user's session on the compositor, without Nova.
      desktop = {
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
          # foot is a window to have.
          systemPackages = [ pkgs.foot ];
        };
        # Only in the interactive shell getty starts: niri-session runs
        # itself again in a non-interactive login shell. Not `exec`: when a
        # test quits the compositor, the shell stays and getty doesn't log
        # in again and restart it.
        programs.bash.loginShellInit = ''
          if [[ $- == *i* && $(tty) == /dev/tty1 ]]; then ${session}; fi
        '';
        systemd = {
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

      # Nova installed the way a NixOS user would: the package and its user unit.
      machine = {
        imports = [ desktop ];
        # grim takes the screenshots, and `quickshell ipc` asks the UI what
        # it got from novad.
        environment.systemPackages = [
          nova
          pkgs.grim
          pkgs.quickshell
        ];
        systemd.packages = [ nova ];
        # NixOS ignores [Install] in packaged units; this is the unit's
        # WantedBy=.
        systemd.user.services.nova.wantedBy = [ "graphical-session.target" ];
      };

      vm = pkgs.testers.runNixOSTest {
        name = "nova-${compositor}";
        nodes = { inherit machine; };
        testScript = ''
          USER = ${builtins.toJSON user}
          UID = ${toString uid}
          QUIT_COMPOSITOR = ${builtins.toJSON "${ipc} ${quitCompositor}"}
          COMPOSITOR = ${builtins.toJSON compositor}
          FOCUS_NEXT_WORKSPACE = ${builtins.toJSON "${ipcEnv} ${steps."focus the next workspace"}"}
        ''
        + builtins.readFile ./lifecycle.py;
      };

      # Not a check: run by hand (`just record`) to refresh the recorded
      # compositor data novad's tests replay. Without Nova, so it records
      # only what the compositor does.
      record = pkgs.testers.runNixOSTest {
        name = "nova-${compositor}-record";
        nodes.machine = {
          imports = [ desktop ];
          environment.systemPackages = [ pkgs.socat ];
        };
        testScript = ''
          USER = ${builtins.toJSON user}
          UID = ${toString uid}
          IPC_ENV = ${builtins.toJSON ipcEnv}
          STREAM = ${builtins.toJSON eventStream}
          QUERIES = ${builtins.toJSON queries}
          STEPS = ${builtins.toJSON steps}
          COMPOSITOR_LOG = ${builtins.toJSON compositorLog}
        ''
        + builtins.readFile ./record.py;
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
        passthru = { inherit vm compare record; };
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
    # Two keyboard layouts, so there's a layout to switch to.
    input {
      kb_layout = us,de
    }
  '';
in
{
  # niri-session runs niri as a systemd unit and activates
  # graphical-session.target, which starts nova.service.
  e2e-niri = mkTest rec {
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
      # Two keyboard layouts, so there's a layout to switch to.
      environment.etc."niri/config.kdl".text = ''
        hotkey-overlay {
          skip-at-startup
        }
        cursor {
          hide-after-inactive-ms 1000
        }
        input {
          keyboard {
            xkb {
              layout "us,de"
            }
          }
        }
      '';
    };
    session = "niri-session";
    ipcEnv = "NIRI_SOCKET=$(echo $XDG_RUNTIME_DIR/niri.*.sock)";
    ipc = "${ipcEnv} niri msg";
    quitCompositor = "action quit --skip-confirmation";
    # What novad sends: the request, then niri answers and streams events.
    eventStream = "(echo '\"EventStream\"'; exec sleep infinity) | socat - UNIX-CONNECT:$NIRI_SOCKET";
    queries = {
      workspaces = "niri msg --json workspaces";
      windows = "niri msg --json windows";
      focused-window = "niri msg --json focused-window";
      keyboard-layouts = "niri msg --json keyboard-layouts";
      outputs = "niri msg --json outputs";
      version = "niri msg --json version";
    };
    compositorLog = "journalctl --user -u niri -n 100 --no-pager";
    steps = {
      "focus the next workspace" = "niri msg action focus-workspace-down";
      "switch the keyboard layout" = "niri msg action switch-layout next";
      "name the workspace" = "niri msg action set-workspace-name web";
      "focus the previous workspace" = "niri msg action focus-workspace-up";
      "focus the other output" = "niri msg action focus-monitor-next";
      "focus the first output" = "niri msg action focus-monitor-previous";
      "close the focused window" = "niri msg action close-window";
    };
  };

  # Plain Hyprland, no uwsm: nothing activates graphical-session.target, so
  # `exec-once = nova` hands the session to systemd and starts the unit.
  e2e-hyprland = mkTest rec {
    compositor = "hyprland";
    config = {
      # Hyprland needs a render node: on the standard VGA it aborts.
      imports = [ virtioGpu ];
      programs.hyprland.enable = true;
    };
    session = "Hyprland --config ${hyprlandConfig}";
    ipcEnv = "HYPRLAND_INSTANCE_SIGNATURE=$(ls $XDG_RUNTIME_DIR/hypr | head -n1)";
    ipc = "${ipcEnv} hyprctl";
    quitCompositor = "dispatch exit";
    eventStream = "socat -U - UNIX-CONNECT:$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock";
    queries = {
      workspaces = "hyprctl -j workspaces";
      monitors = "hyprctl -j monitors";
      activewindow = "hyprctl -j activewindow";
      activeworkspace = "hyprctl -j activeworkspace";
      clients = "hyprctl -j clients";
      devices = "hyprctl -j devices";
      version = "hyprctl -j version";
    };
    compositorLog = "tail -n 100 $XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/hyprland.log";
    steps = {
      # A second monitor, so workspaces live on more than one output.
      "add an output" = "hyprctl output create headless HEADLESS-2";
      "focus the next workspace" = "hyprctl dispatch workspace 2";
      "switch the keyboard layout" = "hyprctl switchxkblayout all next";
      "name the workspace" = "hyprctl dispatch renameworkspace 2 web";
      "focus the previous workspace" = "hyprctl dispatch workspace 1";
      "focus the other output" = "hyprctl dispatch focusmonitor +1";
      "focus the first output" = "hyprctl dispatch focusmonitor -1";
      "close the focused window" = "hyprctl dispatch killactive";
    };
  };
}
