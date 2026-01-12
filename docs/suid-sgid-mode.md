# SUID/SGID mode

> [!WARNING]
> This will be deprecated in a future release, see https://git.pvv.ntnu.no/Projects/muscl/issues/101
>
> We do not recommend you use this mode unless you absolutely have to. The biggest reason why `muscl` was rewritten from scratch
> was to fix an architectural issue that easily caused vulnerabilities due to reliance on SUID/SGID. Although the architecture now
> is more resistant against such vulnerabilities, it is not failsafe.

For backwards compatibility reasons, it is possible to run the program without a daemon by utilizing SUID/SGID.

In order to do this, you should set either the SUID/SGID bit and preferably make the executable owned by a non-privileged user.
If the database is running on the same machine, the user/group will need access to write and read from the database socket.
Otherwise, the only requirement is that the user/group is able to read the config file (typically `/etc/muscl/config.toml`).

Note that the feature flag for SUID/SGID mode is not enabled by default, and is not included in the default deb package.
You will need to compile the program yourself with `--features suid-sgid-mode`.
