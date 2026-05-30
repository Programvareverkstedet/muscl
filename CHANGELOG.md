# Changelog

## v1.0.2

Patch release with an important bug fix

### Notable changes

- Run `FLUSH PRIVILEGES` on the server whenever users modify privileges.
  - You will have to grant `RELOAD` for the muscl admin user on all databases, see the [installation docs](./docs/installation.md) for details.
- Bump dependencies

## v1.0.1

Patch release with some important bug fixes

### Notable changes

- `mysql.db.Host` would usually be unset when creating privileges for users, this should be fixed now.
  - You might have to manually set this field for rows created with the previous version of muscl to have those privileges work properly.
- Fixed an issue where a few select server responses would refuse to serialize properly, leading to an error message: "No response from server"
- The output of various commands is now being sorted.
- Bump dependencies

## v1.0.0 - Initial Release

This is the initial release of `muscl`.

### Features ported from [`mysql-admutils`](https://git.pvv.ntnu.no/Projects/mysql-admutils)

- All commands
- Support for starting internal server with SUID/SGID
- Best-effort CLI interface backwards compatibility (see deviation notes for details)
- Best-effort stdout/stderr output backwards compatibility (see line above)
- Privilege editor

### New features and changes from `mysql-admutils`

- Changed programming language from `C` to `Rust`, for better or for worse
- Combined the functionality of both `mysql-dbadm` and `mysql-useradm` into a single executable.
- Switched to a server+client architecture. With this change comes:
  - Added security against SUID/SGID-related vulnerabilities.
  - Logging and debug information for system administrators.
  - A limitation on the maximum number of connections to the database.
  - A lot of sandboxing and hardening for the server-side, limiting the amount
    of damage that can be done if compromised, and further increasing security.
- Added `--json` flag for several commands
- Added `check-auth` command, for testing whether you are allowed to manage certain databases or users
- Added `lock-user`/`unlock-user` which let's you temporarily disable a database user.
- Added dynamic shell completions, aware of which databases and users exist.
- Changed the name length limit from `32` characters to `64` characters.
- Added `-p`/`--privs` flag for editing privileges using only commandline flags.
  The flag acts similarly to `chmod` with `+` and `-` variants for adding and removing privileges.
  See `muscl edit-privs --help` for more information.
- Changed handling of database user passwords:
  - Prompting for passwords will now hide what you write
  - Allow providing passwords through files and stdin
- Respect `$VISUAL` in addition to `$EDITOR` when launching the privilege editor.
- Use a commented example line in the template for the privilege editor on first use.
- Display the diff before committing privilege changes.
- Generally more detailed error reporting:
  - On entering database or user names you do not own, suggest valid names
  - Instead of silently trimming database/user names when too long, report as error
  - When there are other name validation errors, report exactly what went wrong instead of a generic message
  - Add new errors related to failures inbetween the client and the server
- Package and distribute software:
  - Provide `.deb` packages
  - Provide systemd units
  - Provide nix-flake with packages, overlays and NixOS modules.

### Known deviations from `mysql-admutils`' behaviour

- `--help` output is formatted by clap in a different style.
- `mysql-dbadm edit-perm` uses the new privilege editor implementation. The formatting that
  was used in `mysql-admutils` is no longer present. However, since the editor is purely an
  interactive tool, there shouldn't have been any scripts relying on the old formatting.
- The configuration file is shared for all variants of the program, and `muscl` will use
  its new logic to look for and parse this file. See the example config and
  [installation instructions][installation-instructions] for more information about how to
  configure the software.
- The order in which input is validated might be differ from the original
  (e.g. database ownership checks, invalid character checks, existence checks, ...).
  This means that running the exact same command might lead to different error messages.
- Command-line arguments are de-duplicated. For example, if the user runs
  `mysql-dbadm create user_db1 user_db2 user_db1`, the program will only try to create
  the `user_db1` once. The old program would have attempted to create it twice,
  failing the second attempt.
