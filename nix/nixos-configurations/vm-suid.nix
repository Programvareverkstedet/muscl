{ self, nixpkgs, ... }:
let
  inherit (nixpkgs) lib;
in
nixpkgs.lib.nixosSystem {
  system = "x86_64-linux";
  pkgs = import nixpkgs {
    system = "x86_64-linux";
    overlays = [
      self.overlays.muscl-suid-crane
    ];
  };
  modules = [
    "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix"
    "${nixpkgs}/nixos/tests/common/user-account.nix"

    ({ config, pkgs, ... }: {
      system.stateVersion = config.system.nixos.release;
      virtualisation.graphics = false;

      users = {
        groups = {
          a = { };
          b = { };
          muscl = { };
        };
        users.muscl = {
          isSystemUser = true;
          group = "muscl";
        };
        users.alice.extraGroups = [
          "a"
          "b"
          "wheel"
          "systemd-journal"
        ];
        extraUsers.root.password = "root";
      };

      services.getty.autologinUser = "alice";

      users.motd = ''
        =================================
        Welcome to the muscl SUID/SGID vm!

        Try running:
            ${pkgs.muscl.meta.mainProgram}

        Password for alice is 'foobar'
        Password for root is 'root'

        To exit, press Ctrl+A, then X
        =================================
      '';

      services.mysql = {
        enable = true;
        package = pkgs.mariadb;
        ensureUsers = [
          {
            name = "muscl";
            ensurePermissions = {
              "mysql.*" = "SELECT, INSERT, UPDATE, DELETE";
              "*.*" = "GRANT OPTION, CREATE, DROP";
            };
          }
        ];
      };

      security.wrappers.muscl = {
        owner = "muscl";
        group = "muscl";
        setuid = true;
        source = lib.getExe pkgs.muscl;
      };

      environment.etc."muscl/config.toml".source = (pkgs.formats.toml { }).generate "muscl-config.toml" {
        mysql = {
          username = "muscl";
          socket_path = "/run/mysqld/mysqld.sock";
        };
      };

      programs.vim = {
        enable = true;
        defaultEditor = true;
      };

      environment.systemPackages = with pkgs; [ jq pkgs.muscl ];
    })
  ];
}
