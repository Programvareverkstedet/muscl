{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, crane }:
  let
    inherit (nixpkgs) lib;

    systems = [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
      "armv7l-linux"
    ];

    forAllSystems = f: nixpkgs.lib.genAttrs systems (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          (import rust-overlay)
        ];
      };

      rust-bin = rust-overlay.lib.mkRustBin { } pkgs.buildPackages;
      toolchain = rust-bin.stable.latest.default.override {
        extensions = [ "rust-src" ];
      };
    in f system pkgs toolchain);
  in {
    apps = let
      mkApp = program: description: {
        type = "app";
        program = toString program;
        meta = {
          inherit description;
        };
      };
      mkVm = name: mkApp "${self.nixosConfigurations.${name}.config.system.build.vm}/bin/run-nixos-vm";
    in forAllSystems (system: pkgs: _: {
      muscl = mkApp (lib.getExe self.packages.${system}.muscl) "Run muscl without any setup";
      coverage = mkApp (pkgs.writeShellScript "muscl-coverage" ''
        ${lib.getExe pkgs.python3} -m http.server -d "${self.packages.${system}.coverage}/html"
      '') "Serve code coverage report at http://localhost:8000";

      vm = mkVm "vm" "Start a NixOS VM with muscl and mariadb installed";
      vm-mysql = mkVm "vm-mysql" "Start a NixOS VM with muscl and mysql installed";
      vm-suid = mkVm "vm-suid" "Start a NixOS VM with muscl as SUID/SGID installed";
    });

    nixosConfigurations = {
      vm = import ./nix/nixos-configurations/vm.nix { inherit self nixpkgs; useMariadb = true; };
      vm-mysql = import ./nix/nixos-configurations/vm.nix { inherit self nixpkgs; useMariadb = false; };
      vm-suid = import ./nix/nixos-configurations/vm-suid.nix { inherit self nixpkgs; };
    };

    devShell = forAllSystems (system: pkgs: toolchain: pkgs.mkShell {
      nativeBuildInputs = with pkgs; [
        toolchain
        mariadb.client
        cargo-nextest
        cargo-edit
        cargo-deny
        cargo-deb
        dpkg
      ];

      RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
    });

    overlays = {
      default = self.overlays.muscl;
      muscl = final: prev: {
        inherit (self.packages.${prev.stdenv.hostPlatform.system}) muscl;
      };
      muscl-crane = final: prev: {
        muscl = self.packages.${prev.stdenv.hostPlatform.system}.muscl-crane;
      };
      muscl-suid = final: prev: {
        muscl = self.packages.${prev.stdenv.hostPlatform.system}.muscl-suid;
      };
      muscl-suid-crane = final: prev: {
        muscl = self.packages.${prev.stdenv.hostPlatform.system}.muscl-suid-crane;
      };
    };

    nixosModules = {
      default = self.nixosModules.muscl;
      muscl = import ./nix/module.nix;
    };

    packages = forAllSystems (system: pkgs: _:
      let
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      cargoLock = ./Cargo.lock;
      craneLib = crane.mkLib pkgs;
      src = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          (craneLib.fileset.commonCargoSources ./.)
          ./assets
          ./examples
        ];
      };
    in {
      default = self.packages.${system}.muscl-crane;

      muscl = pkgs.callPackage ./nix/default.nix { inherit cargoToml cargoLock src; };
      muscl-crane = pkgs.callPackage ./nix/default.nix {
        useCrane = true;
        inherit cargoToml cargoLock src craneLib;
      };

      muscl-suid = pkgs.callPackage ./nix/default.nix {
        suidSgidSupport = true;
        inherit cargoToml cargoLock src;
      };
      muscl-suid-crane = pkgs.callPackage ./nix/default.nix {
        useCrane = true;
        suidSgidSupport = true;
        inherit cargoToml cargoLock src craneLib;
      };

      coverage = pkgs.callPackage ./nix/coverage.nix { inherit cargoToml cargoLock src; };
      filteredSource = pkgs.runCommandLocal "filtered-source" { } ''
        ln -s ${src} $out
      '';
    });

    checks = forAllSystems (system: pkgs: _: {
      # NOTE: the non-crane build runs tests during checkPhase
      inherit (self.packages.${system}) muscl muscl-suid;
    });
  };
}
