# Installation and initial configuration

This document contains instructions for the recommended way of installing and configuring muscl.

Note that there are separate instructions for [installing on NixOS](nixos.md) and [installing with SUID/SGID mode](suid-sgid-mode.md).

After installation, you might want to look at the [Administration and further configuration](administration.md) page.

## Installing with deb on Debian

You can install muscl by adding the [PVV apt repository][pvv-apt-repository] and installing the package:

```bash
# Become root (if not already)
sudo -i

# Check the version of your Debian installation
VERSION_CODENAME=$(. /etc/os-release && echo "$VERSION_CODENAME")

# Pull the repository key
curl https://git.pvv.ntnu.no/api/packages/Projects/debian/repository.key -o /etc/apt/keyrings/pvvgit-projects.asc

# Add the repository
echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/pvvgit-projects.asc] https://git.pvv.ntnu.no/api/packages/Projects/debian $VERSION_CODENAME main" | tee -a /etc/apt/sources.list.d/pvv-git.list

# Update package lists
apt update

# Install muscl
apt install muscl
```

> [!NOTE]
> This has been tested on Debian 12 (bookworm) and Debian 13 (trixie) at the time of writing.

## Creating a database user

In order for the daemon to be able to do anything interesting on the MySQL server, it needs
a database user with sufficient privileges. You can create such a user by running the following commands
on the MySQL server as the admin user (or another user with sufficient privileges):

```sql
CREATE USER `muscl`@`localhost` IDENTIFIED BY '<strong_password_here>';
GRANT SELECT, INSERT, UPDATE, DELETE ON `mysql`.* TO `muscl`@`localhost`;
GRANT GRANT OPTION, CREATE, DROP, RELOAD ON *.* TO `muscl`@`localhost`;
FLUSH PRIVILEGES;
```

Make sure to remember the username and password, as we will now need to add them to the muscl configuration.

If your MySQL server is not running on the same host as the muscl server, you will need to replace `localhost` with the appropriate hostname or IP address in the different commands above. Alternatively, you can use `'%`' to allow connections from any host, but this is not recommended.

The configuration already comes preconfigured expecting the database user to be named `muscl`.
If you named it differently, please edit `/etc/muscl/config.toml` accordingly.

muscl will use the `mysql` database to manage users and databases, and the `*.*` privileges to be able to create, drop and grant privileges on arbitrary databases (restricted by the prefix system).

For systemd-based setups, we recommend using `systemd-creds` to provide the database password, see the section below.

## Setting the MySQL password ...

### ... with `systemd-creds`

The Debian package assumes that you will provide the password for `muscl`'s database user with `systemd-creds`.

You can add the password like this:

```bash
# Become root (if not already)
sudo -i

# Unless you already have a working credential store, you need to set it up first
mkdir -p /etc/credstore.encrypted
systemd-creds setup

# Prompt for the muscl MySQL password
read -s MUSCL_MYSQL_PASSWORD
<... enter strong password here>

# Now set the muscl MySQL password
systemd-creds encrypt --name=muscl_mysql_password <(echo "$MUSCL_MYSQL_PASSWORD") /etc/credstore.encrypted/muscl_mysql_password

# Restart the muscl service to pick up the new credential
systemctl daemon-reload
systemctl restart muscl.service
```

If you are running systemd older than version 254 (see `systemctl --version`), you might have to override the service to point to the path of the credential manually, because `ImportCredential=` is not supported. Run `systemctl edit muscl.service` and add the following lines:

```ini
[Service]
LoadCredentialEncrypted=muscl_mysql_password:/etc/credstore.encrypted/muscl_mysql_password
```

### ... without `systemd-creds`

If you do not have systemd, or if you do not want to use `systemd-creds`, you can also set the password in any other file on the system.
Be careful to ensure that the file is not readable by unprivileged users, as it would yield them too much access to the MySQL server.
Edit `/etc/muscl/config.toml` and set the `mysql_password_file` option below `[database]` to point to the file containing the password.

If you are using systemd, you should also create an override to unset the `ImportCredential=` line. Run `systemctl edit muscl.service` and add the following lines:

```ini
[Service]
ImportCredential=
```

## A note on minimum version requirements

The muscl server will work with older versions of systemd, but the recommended version is 254 or newer.

For full landlock support (disabled by default), you need a Linux kernel version 6.7 or newer.

[pvv-apt-repository]: https://git.pvv.ntnu.no/Projects/-/packages/debian/muscl
