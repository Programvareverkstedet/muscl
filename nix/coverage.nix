{
  lib
, stdenvNoCC
, rustPlatform

, cargoToml
, cargoLock
, src
, commitHash
, commitDate
, commitIsDirty

, rust-bin
, cargo-nextest
, grcov
}:

stdenvNoCC.mkDerivation {
  pname = "coverage-${cargoToml.package.name}";
  version = cargoToml.package.version;
  inherit src;

  env = {
    RUSTFLAGS = "-Cinstrument-coverage";
    LLVM_PROFILE_FILE = "target/coverage/%p-%m.profraw";
    GIT_COMMIT = commitHash;
    GIT_COMMIT_DATE = commitDate;
    GIT_DIRTY = lib.boolToString commitIsDirty;
  };

  cargoDeps = rustPlatform.importCargoLock {
    lockFile = cargoLock;
  };

  nativeBuildInputs = [
    rustPlatform.cargoSetupHook
    cargo-nextest
    grcov
    (rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
      extensions = [ "llvm-tools-preview" ];
    }))
  ];

  buildPhase = ''
    runHook preBuild

    export HOME="$(pwd)"

    cargo nextest run --all-features --release --no-fail-fast

    grcov \
      --source-dir . \
      --binary-path ./target/release/deps/ \
      --excl-start 'mod test* \{' \
      --ignore 'tests/*' \
      --ignore "*test.rs" \
      --ignore "*tests.rs" \
      --ignore "*github.com*" \
      --ignore "*libcore*" \
      --ignore "*rustc*" \
      --ignore "*liballoc*" \
      --ignore "*cargo*" \
      -t html \
      -o ./target/coverage/html \
      target/coverage/

    runHook postBuild
  '';

  installPhase = ''
    runHook preBuild

    mv target/coverage $out

    runHook postBuild
  '';

  meta = with lib; {
    license = licenses.mit;
    platforms = platforms.linux ++ platforms.darwin;
  };
}
