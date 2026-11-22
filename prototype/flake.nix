# IMP-369 GPUI prototype: throwaway, never merges. The dev shell builds and
# runs it on RyzenShine: `nix develop ./prototype` from the worktree root.
# nixpkgs is the rev Nova's own flake.lock pins, so the store already has it.
{
  description = "Nova GPUI prototype (IMP-369, throwaway)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/02c087f3fe673c9e317f206a627cb6b910336dbe";

  outputs =
    { nixpkgs, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      # GPUI opens these with dlopen at run time (same list as Nova Files).
      runtimeLibs = with pkgs; [
        vulkan-loader
        wayland
        libxkbcommon
        libGL
        libx11
        libxcb
        libxcursor
        libxi
        libxrandr
      ];
    in
    {
      devShells.x86_64-linux.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          pkg-config
          cmake
          rustPlatform.bindgenHook
        ];
        buildInputs = with pkgs; [
          fontconfig
          freetype
          libxkbcommon
          wayland
          libx11
          libxcb
          zstd
        ];
        packages = with pkgs; [
          cargo
          rustc
          # Capture and measuring.
          grim
          wf-recorder
          ffmpeg
          python3
          jq
          wlr-randr
        ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
      };
    };
}
