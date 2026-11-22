# Nova's features as the Nix modules see them, from the same manifests
# (features/<id>/feature.toml) that `nova setup` and novad read. Picking a
# variant and deciding what's on follow nova-features (framework/features)
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

  # The mode a features.toml `selection` ({ modes }) picks for `manifest`:
  # the one picked when it's one of its modes, else its `default-mode`;
  # null for a feature without modes (Selection::mode).
  modeOf =
    selection: manifest:
    let
      picked = (selection.modes or { }).${manifest.id} or null;
    in
    if lib.elem picked (manifest.modes or [ ]) then picked else manifest.default-mode or null;

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

  # A features.toml for `switches` (programs.nova.features) and the UI
  # program `ui` (programs.nova.ui): which features differ from their
  # default, the mode of each that's on and has modes, and the UI program
  # when it isn't Quickshell.
  selectionFile =
    ui: switches:
    let
      on = lib.attrNames (lib.filterAttrs (_: feature: feature.enable) switches);
      enabled = lib.attrNames (lib.filterAttrs (id: m: !isDefault m && lib.elem id on) manifests);
      disabled = lib.attrNames (lib.filterAttrs (id: m: isDefault m && !lib.elem id on) manifests);
      modes = lib.mapAttrs (id: _: switches.${id}.mode) (
        lib.filterAttrs (id: m: lib.elem id on && (m.modes or [ ]) != [ ]) manifests
      );
    in
    ''
      # Written by Nova's Nix module from programs.nova.features (or
      # programs.nova.featuresFile). Change features there and rebuild, which
      # also installs what they need.
      enabled = ${builtins.toJSON enabled}
      disabled = ${builtins.toJSON disabled}
    ''
    # Only when it isn't the default, so the file stays one an older Nova
    # reads.
    + lib.optionalString (ui != "quickshell") "ui = ${builtins.toJSON ui}\n"
    + ''

      [modes]
    ''
    + lib.concatStrings (lib.mapAttrsToList (id: mode: "${id} = ${builtins.toJSON mode}\n") modes);
}
