# Nova's features as the Nix modules see them, from the same manifests
# (features/<id>/feature.toml) that `nova setup` and novad read. Picking a
# variant and deciding what's on follow nova-features (crates/nova-features)
# exactly, so the packages Nix installs are the ones novad checks for.
{ lib }:
let
  dir = ../../features;

  # Every manifest, by id.
  manifests =
    lib.mapAttrs (id: _: builtins.fromTOML (builtins.readFile (dir + "/${id}/feature.toml")))
      (
        lib.filterAttrs (
          id: type: type == "directory" && builtins.pathExists (dir + "/${id}/feature.toml")
        ) (builtins.readDir dir)
      );

  isDefault = manifest: manifest.default or false;

  # The variant for `gpu` (null when unknown): the first one by name whose
  # `when.gpu` is that GPU, else `default` (Manifest::variant_for).
  variantFor =
    manifest: gpu:
    let
      variants = manifest.variants or { };
      matching = lib.filter (
        name: name != "default" && gpu != null && (variants.${name}.when.gpu or null) == gpu
      ) (lib.attrNames variants);
    in
    if matching != [ ] then variants.${lib.head matching} else variants.default or null;

  # nixpkgs attribute names the feature needs: its variant's `nix` list, else
  # `default`'s (Manifest::packages).
  nixNames =
    manifest: gpu:
    let
      lists = map (variant: variant.nix or null) [
        (variantFor manifest gpu)
        (manifest.variants.default or null)
      ];
    in
    lib.findFirst (list: list != null) [ ] lists;
in
{
  inherit manifests;

  # The ids of the features whose programs.nova.features.<id>.enable is set.
  # Every feature can be turned off, the bar too.
  onIds = switches: lib.attrNames (lib.filterAttrs (_: feature: feature.enable) switches);

  # The GPU a NixOS configuration drives, as far as features care: only
  # NVIDIA's driver changes what they install.
  gpuOf =
    nixos: if lib.elem "nvidia" (nixos.services.xserver.videoDrivers or [ ]) then "nvidia" else null;

  # Whether a features.toml `selection` ({ enabled, disabled }) turns
  # `manifest` on (Selection::is_on).
  isOn =
    selection: manifest:
    !(lib.elem manifest.id (selection.disabled or [ ]))
    && (isDefault manifest || lib.elem manifest.id (selection.enabled or [ ]));

  # The packages of the features in `ids`, from `pkgs`. A name like
  # `kdePackages.plasma-workspace` is an attribute path.
  packagesFor =
    pkgs: gpu: ids:
    lib.unique (
      lib.concatMap (
        id: map (name: lib.getAttrFromPath (lib.splitString "." name) pkgs) (nixNames manifests.${id} gpu)
      ) ids
    );

  # Whether the features in `ids` run gpu-screen-recorder, whose capture
  # helper needs NixOS's `programs.gpu-screen-recorder`.
  usesGpuScreenRecorder =
    gpu: ids: lib.any (id: lib.elem "gpu-screen-recorder" (nixNames manifests.${id} gpu)) ids;

  # Nova Files' desktop entry and its hicolor icon (features/files), running
  # `package`'s nova, for the systems where the `files` feature is on: Nova
  # Files is then listed with the apps, and can be the default for folders.
  filesDesktop =
    pkgs: package:
    pkgs.runCommand "nova-files-desktop" { } ''
      install -Dm644 ${package}/share/nova/features/files/nova-files.desktop \
        $out/share/applications/nova-files.desktop
      substituteInPlace $out/share/applications/nova-files.desktop \
        --replace-fail "Exec=nova " "Exec=${lib.getExe package} "
      install -Dm644 ${package}/share/nova/features/files/nova-files.svg \
        $out/share/icons/hicolor/scalable/apps/nova-files.svg
    '';

  # Nova's file chooser backend for xdg-desktop-portal (the `files`
  # feature), for xdg.portal.extraPortals: share/xdg-desktop-portal/portals.
  filesPortal =
    pkgs: package:
    pkgs.runCommand "nova-files-portal" { } ''
      install -Dm644 ${package}/share/nova/features/files/nova.portal \
        $out/share/xdg-desktop-portal/portals/nova.portal
    '';

  # The portal config that makes Nova the file chooser
  # (xdg.portal.config.<desktop>), on the compositors Nova supports. The
  # portal reads one config file per desktop, so this one also carries the
  # compositor's own choices from its packaged <desktop>-portals.conf
  # (niri 25.08, Hyprland 0.50), which a user's own settings override.
  # What chose the file chooser before stays as the fallback.
  portalConfig = lib.mkMerge [
    (lib.mapAttrs (_: lib.mapAttrs (_: lib.mkDefault)) {
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
    })
    (lib.genAttrs [ "niri" "hyprland" ] (_: {
      "org.freedesktop.impl.portal.FileChooser" = [
        "nova"
        "gtk"
      ];
    }))
  ];

  # A features.toml for `on` (the ids that are on): what differs from each
  # feature's default.
  selectionFile =
    on:
    let
      enabled = lib.attrNames (lib.filterAttrs (id: m: !isDefault m && lib.elem id on) manifests);
      disabled = lib.attrNames (lib.filterAttrs (id: m: isDefault m && !lib.elem id on) manifests);
    in
    ''
      # Written by Nova's Nix module from programs.nova.features (or
      # programs.nova.featuresFile). Change features there and rebuild, which
      # also installs what they need.
      enabled = ${builtins.toJSON enabled}
      disabled = ${builtins.toJSON disabled}
    '';
}
