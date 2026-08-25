{ pkgs, self }:
{
  basic-mysql = pkgs.testers.runNixOSTest (import ./basic.nix { inherit self; useMariadb = false; });
  basic-mariadb = pkgs.testers.runNixOSTest (import ./basic.nix { inherit self; useMariadb = true; });
}
