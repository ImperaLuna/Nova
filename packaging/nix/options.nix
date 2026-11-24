# `programs.nova`, shared by the NixOS and home-manager modules: Nova, and a
# switch per feature that also installs what the feature needs.
#
# Turn features on or off either with programs.nova.features.<id>.enable,
# or with programs.nova.featuresFile, Nova's own features.toml (the same
# file `nova setup` reads on other distros). A feature with modes also has
# programs.nova.features.<id>.mode. The switches start from that file, so
# setting one still wins over it.
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
  fileSelection =
    if cfg.featuresFile == null then { } else builtins.fromTOML (builtins.readFile cfg.featuresFile);
in
{
  imports = [
    (lib.mkRemovedOptionModule [ "programs" "nova" "ui" ] "Nova has one UI now.")
  ];

  options.programs.nova = {
    enable = lib.mkEnableOption "Nova, a desktop shell for Wayland compositors";

    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${pkgs.stdenv.hostPlatform.system}.nova;
      defaultText = lib.literalExpression "nova.packages.\${system}.nova";
      description = "The Nova package.";
    };

    novaFilesPackage = lib.mkOption {
      type = lib.types.package;
      default = packages.${pkgs.stdenv.hostPlatform.system}.nova-files;
      defaultText = lib.literalExpression "nova.packages.\${system}.nova-files";
      description = "The Nova Files app, which the `nova-files` feature installs (on NixOS through `programs.nova-files.package`).";
    };

    uiPackage = lib.mkOption {
      type = lib.types.package;
      default = packages.${pkgs.stdenv.hostPlatform.system}.nova-ui;
      defaultText = lib.literalExpression "nova.packages.\${system}.nova-ui";
      description = "`nova-ui`, the program that draws Nova's UI.";
    };

    featuresFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = lib.literalExpression "./nova/features.toml";
      description = ''
        Nova's features.toml (`enabled = [...]`, `disabled = [...]`, and
        `[modes]`), as on other distros. When set, each feature's switch and
        mode start from it.
      '';
    };

    gpu = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.enum [
          "nvidia"
          "amd"
          "intel"
        ]
      );
      default = null;
      description = ''
        The GPU, for features that install something else per GPU (screen
        recording uses gpu-screen-recorder on NVIDIA). The NixOS module sets
        it from services.xserver.videoDrivers.
      '';
    };

    fonts.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to install the fonts Nova's look is drawn in: Inter,
        JetBrains Mono and Instrument Serif. Without them both UIs draw
        fontconfig's nearest match (often DejaVu).
      '';
    };

    features = lib.mapAttrs (
      _: manifest:
      lib.mkOption {
        type = lib.types.submodule {
          options = {
            enable = lib.mkOption {
              type = lib.types.bool;
              default = features.isOn fileSelection manifest;
              defaultText = lib.literalMD "`${
                lib.boolToString (manifest.default or false)
              }`, or what `programs.nova.featuresFile` says";
              description = "Whether Nova's ${manifest.name} feature is on: ${manifest.description}";
            };
          }
          // lib.optionalAttrs (manifest ? modes) {
            mode = lib.mkOption {
              type = lib.types.enum manifest.modes;
              default = features.modeOf fileSelection manifest;
              defaultText = lib.literalMD "`\"${manifest.default-mode}\"`, or what `programs.nova.featuresFile` says";
              description = "Which mode Nova's ${manifest.name} feature runs in while it's on.";
            };
          };
        };
        default = { };
        description = "${manifest.name}.";
      }
    ) features.manifests;
  };
}
