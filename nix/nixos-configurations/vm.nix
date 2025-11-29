{ self, nixpkgs, useMariadb ? true, ... }:
nixpkgs.lib.nixosSystem {
  system = "x86_64-linux";
  pkgs = import nixpkgs {
    system = "x86_64-linux";
    overlays = [
      self.overlays.muscl-crane
    ];
  };
  modules = [
    "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix"
    "${nixpkgs}/nixos/tests/common/user-account.nix"

    self.nixosModules.default

    ({ config, pkgs, ... }: {
      system.stateVersion = config.system.nixos.release;
      virtualisation.graphics = false;

      users = {
        groups = {
          a = { };
          b = { };
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
        Welcome to the muscl vm!

        Try running:
            ${config.services.muscl.package.meta.mainProgram}

        Password for alice is 'foobar'
        Password for root is 'root'

        To exit, press Ctrl+A, then X
        =================================
      '';

      services.mysql = {
        enable = true;
        package = if useMariadb then pkgs.mariadb else pkgs.mysql84;
        dataDir = if useMariadb then "/var/lib/mariadb" else "/var/lib/mysql";
      };
      services.muscl = {
        enable = true;
        logLevel = "trace";
        createLocalDatabaseUser = true;
        authHandler = ''
          def process_request(
              username: str,
              groups: list[str],
              resource_type: str,
              resource: str,
          ) -> bool:
              if resource_type == "database":
                  if resource.startswith(username) or any(
                      resource.startswith(group) for group in groups
                  ):
                      return True
              elif resource_type == "user":
                  if resource.startswith(username) or any(
                      resource.startswith(group) for group in groups
                  ):
                      return True
              return False
        '';
      };

      programs.vim = {
        enable = true;
        defaultEditor = true;
      };

      environment.systemPackages = with pkgs; [ jq ];
    })
  ];
}
