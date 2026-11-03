{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "polaris-tui";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../examples
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = [
    "--package"
    "polaris-tui"
  ];

  meta = {
    description = "Terminal dashboard rendering Grafana data on a Linux console";
    license = lib.licenses.mit;
    mainProgram = "polaris-tui";
    platforms = lib.platforms.linux;
  };
}
