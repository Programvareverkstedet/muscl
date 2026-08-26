# Compatibility mode with [mysql-admutils](https://git.pvv.ntnu.no/Projects/mysql-admutils)

If you enable the `mysql-admutils-compatibility` feature flag when [compiling][compiling] (enabled by default for now), the output directory will contain two symlinks to the `muscl` binary: `mysql-dbadm` and `mysql-useradm`. When you run either of the symlinks, the program will enter a compatibility mode that mimics the behaviour of the corresponding program from the `mysql-admutils` package. These tools try to replicate the behaviour of the original programs as closely as possible.

```bash
cargo build
./target/debug/mysql-dbadm --help
./target/debug/mysql-useradm --help
```

These symlinks are also included in the deb packages by default.

### Known deviations from `mysql-admutils`' behaviour

There are some differences between the original programs and the compatibility mode in `muscl`.
The known ones are:

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
- Editing privileges for a database user pair requires the calling user to *own* both
  the database and the user. The original program only required ownership of the database.
  See [#228](https://git.pvv.ntnu.no/Projects/muscl/issues/228) for more information.

One detail that might be considered a difference but, is that the compatibility mode supports
command line completions when the user presses tab. This is not a feature of the original programs,
but it does not change any of the previous behaviour either.

[compiling]: ./compiling.md
[installation-instructions]: ./installation.md
