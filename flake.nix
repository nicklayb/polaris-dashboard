{
  description = "Polaris: browser-less Grafana dashboard for small rack displays";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem =
        function: nixpkgs.lib.genAttrs systems (system: function nixpkgs.legacyPackages.${system});
    in
    {
      packages = forEachSystem (pkgs: {
        polaris-tui = pkgs.callPackage ./nix/package.nix { };
        polaris-tui-aarch64 = pkgs.pkgsCross.aarch64-multiplatform.callPackage ./nix/package.nix { };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.polaris-tui;
      });

      nixosModules.default = import ./nix/module.nix self;

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            just
            foot
            jq
          ];
          # Only used by scripts/run-sized.sh to preview with the kmscon font.
          POLARIS_FONTCONFIG = pkgs.makeFontsConf { fontDirectories = [ pkgs.cascadia-code ]; };
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });

      checks = forEachSystem (pkgs: {
        polaris-tui = self.packages.${pkgs.stdenv.hostPlatform.system}.polaris-tui;
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt);
    };
}
