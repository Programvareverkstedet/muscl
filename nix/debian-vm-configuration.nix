{ nix-vm-test, nixpkgs, system, pkgs, ... }:
let
  image = nix-vm-test.lib.${system}.debian.images."13";

  generic = import "${nix-vm-test}/generic" { inherit pkgs nixpkgs; inherit (pkgs) lib; };

  makeVmTestForImage =
    image:
    {
      testScript,
      sharedDirs ? {},
      diskSize ? null,
      config ? { }
    }:
    generic.makeVmTest {
      inherit
        system
        testScript
        sharedDirs;
      image = nix-vm-test.lib.${system}.debian.prepareDebianImage {
        inherit diskSize;
        hostPkgs = pkgs;
        originalImage = image;
      };
      machineConfigModule = config;
    };

    vmTest = makeVmTestForImage image {
       diskSize = "10G";
       sharedDirs = {
         debDir = {
           source = "${./.}";
           target = "/mnt";
         };
       };
       testScript = ''
         vm.wait_for_unit("multi-user.target")
         vm.succeed("apt-get update && apt-get -y install mariadb-server build-essential curl")
         vm.succeed("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y")
         vm.succeed("source /root/.cargo/env && cargo install cargo-deb")
         vm.succeed("cp -r /mnt /root/src && chmod -R +w /root/src")
         vm.succeed("source /root/.cargo/env && cd /root/src && ./create-deb.sh")
       '';
       config.nodes.vm = {
         virtualisation.memorySize = 8192;
         virtualisation.cpus = 4;
       };
    };
in vmTest.driverInteractive
