# The fonts Nova's look is drawn in, which both modules install while
# `programs.nova.fonts.enable` is on: Inter for running text, JetBrains Mono
# for numbers, Instrument Serif for display text. Their family names are
# `nova-theme-info`'s FONT_* constants, which the look tokens name; the
# `fonts` flake check holds that each name matches a face from this list.
#
# Instrument Serif has no nixpkgs package of its own: it is only inside
# `google-fonts`, whose override for one family isn't in the binary cache and
# builds from the whole ~2.3 GB source. So its two files come from Google
# Fonts' repository instead, at the commit nixpkgs' google-fonts uses.
{ pkgs }:
let
  googleFontsRev = "5174b3333331c966c38f4355d50b03ca1c1df2f9";
  fromGoogleFonts =
    file: hash:
    pkgs.fetchurl {
      url = "https://raw.githubusercontent.com/google/fonts/${googleFontsRev}/ofl/instrumentserif/${file}";
      inherit hash;
    };
  instrument-serif = pkgs.stdenvNoCC.mkDerivation {
    pname = "instrument-serif";
    version = "1.000";
    dontUnpack = true;
    installPhase = ''
      install -Dm444 ${fromGoogleFonts "InstrumentSerif-Regular.ttf" "sha256-SY79Rh9t38t6ERv5pWVwnSCF1IIB1QHq2WDZPoT/u4g="} $out/share/fonts/truetype/InstrumentSerif-Regular.ttf
      install -Dm444 ${fromGoogleFonts "InstrumentSerif-Italic.ttf" "sha256-CJObi99TSv7CSuDvXgP5SJQM2aj+COf+y60EDmIyc4U="} $out/share/fonts/truetype/InstrumentSerif-Italic.ttf
      install -Dm444 ${fromGoogleFonts "OFL.txt" "sha256-Ep7XYYlZcWlZ8pQf3VtJ4K1ubB14cmdheGoAJT2GVSE="} $out/share/licenses/instrument-serif/OFL.txt
    '';
    meta = {
      description = "Instrument Serif, the display face of Nova's look";
      homepage = "https://github.com/Instrument/instrument-serif";
      license = pkgs.lib.licenses.ofl;
      platforms = pkgs.lib.platforms.all;
    };
  };
in
[
  pkgs.inter
  pkgs.jetbrains-mono
  instrument-serif
]
