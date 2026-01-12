[![Coverage](https://pages.pvv.ntnu.no/Projects/muscl/main/coverage/badges/for_the_badge.svg)](https://pages.pvv.ntnu.no/Projects/muscl/main/coverage/)
[![Docs](https://img.shields.io/badge/rust_docs-blue?style=for-the-badge&logo=rust)](https://pages.pvv.ntnu.no/Projects/muscl/main/docs/muscl/)

# muscl 💪

Dropping DBs (dumbbells) and having MySQL spasms since 2024

## What is this?

`muscl` is a secure MySQL administration tool for multi-user systems.
It allows unprivileged users to manage their own databases and database users without granting them direct access to the MySQL server.
Authorization is handled by a prefix-based model tied to Unix users and groups, making it ideal for shared hosting environments, like university servers, tilde servers, or similar.

When a user requests an administrative operation, the `muscl` daemon verifies authenticates the user through unix socket peer credentials,
and then checks the requested item name against the user's username and group list for authorization.
The default authorization mechanism only allows the user to manage items prefixed with either their username or a group name.
For example, a user would be allowed to manage items like `<user>_mydb`, `<user>_mydbuser`, or `<group>_myotherdb`.

The available administrative operations include:

```bash
# Creating, listing, modifying, and deleting databases and database users
muscl create-db user_testdb
muscl create-user user_testuser --password strongpassword
muscl show-db
muscl drop-db group_projectdb

# Modifying privileges for a database user on a database
muscl edit-privs user_testdb user_testuser +suid
muscl edit-privs -p user_testdb:user_testuser:A -p group_projectdb:otheruser:-d
muscl show-privs --json

# Changing the passwords of the database users
muscl passwd-user user_testuser
muscl passwd-user user_otheruser --stdin <<<"hunter2"

# Locking and unlocking database users
muscl lock-user user_testuser
muscl unlock-user user_testuser

# And more...
```

The software is designed to be run as a client and a server. The clients are run by the unprivileged users,
and does not have direct access to the MySQL server. Instead, they communicate with the muscl server
over a IPC, which then performs the requested operations on behalf of the clients.

## Documentation

- [Installation and initial configuration](docs/installation.md)
- [Administration and further configuration](docs/administration.md)
- [Development and testing](docs/development.md)
- [Compiling and packaging](docs/compiling.md)
- [Compatibility mode with mysql-admutils](docs/mysql-admutils-compatibility.md)
- [Use with NixOS](docs/nixos.md)
- [SUID/SGID mode](docs/suid-sgid-mode.md)

## History

This is a rewrite of an older piece of software called [mysql-admutils](https://git.pvv.ntnu.no/Projects/mysql-admutils).
Programvareverkstedet used this a lot back in the day, and it was great.
But it had some security issues inherent to the software design, particularly related to the use of SUID/SGID.
We tried patching it multiple times, but the issue kept popping up again in different ways.
The rewrite was intended to iron this issue out completely by splitting the software into two pieces - a client and a server.
As far as we know, this was successful, and it is unlikely for similar issues to resurface in the future.
