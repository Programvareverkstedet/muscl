{ self, useMariadb ? false, ... }:
{
  name = "muscl-basic-${if useMariadb then "mariadb" else "mysql"}";

  nodes.machine = { pkgs, lib, ... }: {
    imports = [ self.nixosModules.default ];

    services.mysql = {
      enable = true;
      package = if useMariadb then pkgs.mariadb else pkgs.mysql84;

      # mariadb ships anonymous ''@localhost / ''@<hostname> accounts.
      # Yeet them.
      initialScript = lib.mkIf useMariadb (pkgs.writeText "muscl-test-init.sql" ''
        DELETE FROM mysql.user WHERE user = ''';
        FLUSH PRIVILEGES;
      '');
    };

    services.muscl = {
      enable = true;
      logLevel = "trace";
      createLocalDatabaseUser = true;
    };

    users.groups.friends = { };
    users.users.alice = {
      isNormalUser = true;
      extraGroups = [ "friends" ];
    };
    users.users.bob.isNormalUser = true;

    environment.systemPackages = [ pkgs.mariadb.client ];
  };

  testScript = ''
    import json
    from typing import Any


    def muscl(user: str, args: str) -> str:
        return machine.succeed(f"su - {user} -c 'muscl {args}'")


    def muscl_json(
        user: str,
        args: str,
        key: str | None = None,
        expect_status: str = "success",
        error_type: str | None = None,
    ) -> Any:
        _, output = machine.execute(f"su - {user} -c 'muscl {args} --json'")
        result = json.loads(output)
        if key is None:
            return result
        entry = result[key]
        assert entry["status"] == expect_status, entry
        if error_type is not None:
            assert error_type in entry.get("type", ""), entry
        return entry.get("value")


    def mysql_connect_as(username: str, password: str, extra: str = "") -> str:
        return machine.succeed(
            f"mysql --socket=/run/mysqld/mysqld.sock -u {username} -p{password} {extra} -e 'SELECT 1'"
        )


    def mysql_connect_as_fails(username: str, password: str, extra: str = "") -> str:
        return machine.fail(
            f"mysql --socket=/run/mysqld/mysqld.sock -u {username} -p{password} {extra} -e 'SELECT 1'"
        )


    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("mysql.service")
    machine.wait_for_unit("muscl.socket")

    with subtest("create-db"):
        muscl_json("alice", "create-db alice_testdb", "alice_testdb")
        muscl_json("alice", "create-db friends_shared", "friends_shared")
        muscl_json(
            "bob",
            "create-db alice_testdb2",
            "alice_testdb2",
            expect_status="error",
            error_type="illegal-prefix",
        )
        muscl_json(
            "bob",
            "create-db friends_shared2",
            "friends_shared2",
            expect_status="error",
            error_type="illegal-prefix",
        )

    with subtest("show-db"):
        result = muscl_json("alice", "show-db alice_testdb friends_shared")
        assert result["alice_testdb"]["status"] == "success", result
        assert result["friends_shared"]["status"] == "success", result

    with subtest("create-user"):
        muscl_json("alice", "create-user alice_testuser", "alice_testuser")

    with subtest("passwd-user"):
        machine.succeed(
            "su - alice -c 'muscl passwd-user alice_testuser --stdin <<<Sup3rSecret1x'"
        )
        mysql_connect_as("alice_testuser", "Sup3rSecret1x")

    with subtest("show-user"):
        value = muscl_json("alice", "show-user alice_testuser", "alice_testuser")
        assert value["has_password"], value
        assert not value["is_locked"], value
        assert value["database_count"] == 0, value

        mysql_connect_as_fails("alice_testuser", "Sup3rSecret1x", "-D alice_testdb")

    with subtest("edit-privs"):
        muscl("alice", "edit-privs alice_testdb alice_testuser A --yes")
        machine.succeed(
            "mysql --socket=/run/mysqld/mysqld.sock -u alice_testuser -pSup3rSecret1x "
            "-D alice_testdb -e "
            "'CREATE TABLE t (id INT); INSERT INTO t VALUES (1); SELECT * FROM t;'"
        )

    with subtest("show-privs"):
        value = muscl_json("alice", "show-privs alice_testdb", "alice_testdb")
        privs = value["alice_testuser"][0]
        assert privs["select_priv"], privs
        assert privs["create_priv"], privs
        assert privs["insert_priv"], privs

    with subtest("lock-user"):
        muscl_json("alice", "lock-user alice_testuser", "alice_testuser")
        mysql_connect_as_fails("alice_testuser", "Sup3rSecret1x")

        value = muscl_json("alice", "show-user alice_testuser", "alice_testuser")
        assert value["is_locked"], value

    with subtest("unlock-user"):
        muscl_json("alice", "unlock-user alice_testuser", "alice_testuser")
        mysql_connect_as("alice_testuser", "Sup3rSecret1x")

        value = muscl_json("alice", "show-user alice_testuser", "alice_testuser")
        assert not value["is_locked"], value

    with subtest("drop-user"):
        muscl_json("alice", "drop-user alice_testuser --yes", "alice_testuser")

    with subtest("drop-db"):
        # bob still isn't authorized to drop alice's or the group's databases.
        muscl_json(
            "bob",
            "drop-db alice_testdb --yes",
            "alice_testdb",
            expect_status="error",
            error_type="illegal-prefix",
        )
        muscl_json(
            "bob",
            "drop-db friends_shared --yes",
            "friends_shared",
            expect_status="error",
            error_type="illegal-prefix",
        )

        muscl_json("alice", "drop-db alice_testdb --yes", "alice_testdb")
        muscl_json("alice", "drop-db friends_shared --yes", "friends_shared")
  '';
}
