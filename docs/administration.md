# Administration and further configuration

This page describes some additional configuration options and administration tasks for muscl.

## Configuring group denylists

In `/etc/muscl/muscl.conf`, you will find an option below `[authorization]` named `group_denylist_file`,
which points to `/etc/muscl/group_denylist.txt` by default.

In this file, you can add unix group names or GIDs to disallow the groups from being used as prefixes.

The deb package comes with a default denylist that disallows some common system groups.

The format of the file is one group name or GID per line. Lines starting with `#` and empty lines are ignored.

```
# Disallow using the 'root' group as a prefix
gid:0

# Disallow using the 'adm' group as a prefix
group:adm
```

> [!NOTE]
> If a user is named the same as a disallowed group, that user will still be able to use their username as a prefix.

## Configuring logging

By default, muscl logs to the systemd journal when run as a systemd service,
and also limits the log level to `info`. You can request more verbose logging
by appending `-v` flags to the `ExecStart=` line in the systemd service file.

To do this on a system where muscl was installed using a package, you can override
the service like this:

```bash
sudo systemctl edit muscl.service
```

This will open an editor where you can add the following lines:

```ini
[Service]
ExecStart=
ExecStart=/usr/bin/muscl-server -v ...
```

> [!NOTE]
> The first `ExecStart=` line is necessary to clear the previous value, as systemd
> interprets multiple `ExecStart=` lines as a list of commands to run in sequence.

You set either `-v` or `-vv` for `debug` and `trace` logging, respectively.

> [!WARNING]
> Be careful when enabling trace logging on production systems, as it might log
> passwords and credentials in plaintext.

## Querying logs in the systemd journal

Although invisible if you just run `journalctl -u muscl.service`, muscl adds a set of so-called
"fields" to its log entries to make it easier to filter and search them.

Here are some examples of how you can filter logs using `journalctl`:

```bash
# Show only logs related to a specific user
journalctl -eu muscl F_USER="<username>"
journalctl -eu muscl F_USER=johndoe

# Show only logs for a specific command types
journalctl -eu muscl F_COMMAND="<operation>"
journalctl -eu muscl F_COMMAND=create-db

# Show logs emitted for a specific session id
journalctl -eu muscl F_SESSION_ID="<session-id>"
journalctl -eu muscl F_SESSION_ID=123

# Show all of these fields together with the log message in a json format
journalctl --output json-pretty --output-fields MESSAGE,F_USER,F_COMMAND,F_SESSION_ID -eu muscl
```

See [`journalctl(1)`][journalctl_1] and [`systemd.journal-fields(7)`][systemd_journal-fields_7] for more information.

> [!NOTE]
> Please note that the commands are not 1-1 mapped to muscl subcommands.
> Rather, they are the available requests in the protocol used between the muscl client and server.
> These requests will often have the same name as the subcommands, but this is not always the case.

[journalctl_1]: https://man7.org/linux/man-pages/man1/journalctl.1.html
[systemd_journal-fields_7]: https://man7.org/linux/man-pages/man7/systemd.journal-fields.7.html
