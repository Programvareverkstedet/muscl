{
  lib
, rustPlatform
, stdenv
, installShellFiles
, versionCheckHook

, cargoToml
, cargoLock
, src

, useCrane ? false
, craneLib ? null
, suidSgidSupport ? false
}:
let
  mainProgram = (lib.head cargoToml.bin).name;
  buildFunction = if useCrane then craneLib.buildPackage else rustPlatform.buildRustPackage;

  pnameCraneSuffix = lib.optionalString useCrane "-crane";
  pnameSuidSuffix = lib.optionalString suidSgidSupport "-suid";
  pname = "${cargoToml.package.name}${pnameSuidSuffix}${pnameCraneSuffix}";

  rustPlatformArgs = {
    buildType = "release-lto";
    buildFeatures = lib.optional suidSgidSupport "suid-sgid-mode";
    cargoLock.lockFile = cargoLock;

    doCheck = true;
    useNextest = true;
    nativeCheckInputs = [
      versionCheckHook
    ];
    cargoCheckFeatures = lib.optional suidSgidSupport "suid-sgid-mode";

    postCheck = lib.optionalString (stdenv.buildPlatform.system == stdenv.hostPlatform.system && suidSgidSupport) ''
      ./target/${stdenv.hostPlatform.rust.rustcTarget}/release/muscl --version | grep "SUID/SGID mode: enabled"
    '';
  };

  craneArgs = {
    cargoLock = cargoLock;
    cargoExtraArgs = lib.escapeShellArgs [ "--features" (lib.concatStringsSep "," (lib.optional suidSgidSupport "suid-sgid-mode")) ];
    cargoArtifacts = craneLib.buildDepsOnly {
      inherit pname;
      inherit (cargoToml.package) version;
      src = lib.fileset.toSource {
        root = ../.;
        fileset = lib.fileset.unions [
          (craneLib.fileset.cargoTomlAndLock ../.)
        ];
      };

      cargoLock = cargoLock;
    };
  };
in
buildFunction ({
  inherit pname;
  inherit (cargoToml.package) version;
  inherit src;

  nativeBuildInputs = [ installShellFiles ];
  postInstall = let
    installShellCompletions = lib.mapCartesianProduct ({ shell, command }: ''
      (
        export PATH="$out/bin:$PATH"
        export COMPLETE="${shell}"
        "${command}" > "$TMP/${command}.${shell}"
      )
      # See https://github.com/clap-rs/clap/issues/1764
      sed -i 's/muscl/${command}/g' "$TMP/${command}.${shell}"
      installShellCompletion "--${shell}" --cmd "${command}" "$TMP/${command}.${shell}"
    '') {
      shell = [ "bash" "zsh" "fish" ];
      command = [ "muscl" "mysql-dbadm" "mysql-useradm" ];
    };
  in ''
    ln -sr "$out/bin/muscl" "$out/bin/mysql-dbadm"
    ln -sr "$out/bin/muscl" "$out/bin/mysql-useradm"

    ${lib.concatStringsSep "\n" installShellCompletions}

    install -Dm444 assets/systemd/muscl.socket -t "$out/lib/systemd/system"
    install -Dm644 assets/systemd/muscl.service -t "$out/lib/systemd/system"
    substituteInPlace "$out/lib/systemd/system/muscl.service" \
      --replace-fail '/usr/bin/muscl-server' "$out/bin/muscl-server"

    mkdir -p "$out/share/muscl"
    cp -r examples "$out/share/muscl"
  '';

  meta = with lib; {
    license = licenses.mit;
    platforms = platforms.linux ++ platforms.darwin;
    inherit mainProgram;
  };
}
//
(if useCrane then craneArgs else rustPlatformArgs)
)
