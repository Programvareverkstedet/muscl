use anyhow::Context;
use indoc::indoc;
use nix::unistd::{Group as LibcGroup, User as LibcUser};

#[cfg(not(target_os = "macos"))]
use std::ffi::CString;
use std::fmt;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/muscl/config.toml";
pub const DEFAULT_SOCKET_PATH: &str = "/run/muscl/muscl.sock";

pub const ASCII_BANNER: &str = indoc! {
  r"
                                __
     ____ ___  __  ____________/ /
    / __ `__ \/ / / / ___/ ___/ /
   / / / / / / /_/ (__  ) /__/ /
  /_/ /_/ /_/\__,_/____/\___/_/
  "
};

pub const KIND_REGARDS: &str = concat!(
    "Hacked together by yours truly, Programvareverkstedet <projects@pvv.ntnu.no>\n",
    "If you experience any bugs or turbulence, please give us a heads up :)",
);

/// TODO: store and display UID
#[derive(Debug, Clone)]
pub struct UnixUser {
    pub username: String,
    pub groups: Vec<String>,
}

impl fmt::Display for UnixUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.username)
    }
}

// TODO: these functions are somewhat critical, and should have integration tests

#[cfg(target_os = "macos")]
fn get_unix_groups(_user: &LibcUser) -> anyhow::Result<Vec<LibcGroup>> {
    // Return an empty list on macOS since there is no `getgrouplist` function
    Ok(vec![])
}

#[cfg(not(target_os = "macos"))]
fn get_unix_groups(user: &LibcUser) -> anyhow::Result<Vec<LibcGroup>> {
    let user_cstr =
        CString::new(user.name.as_bytes()).context("Failed to convert username to CStr")?;
    let groups = nix::unistd::getgrouplist(&user_cstr, user.gid)?
        .iter()
        .filter_map(|gid| match LibcGroup::from_gid(*gid) {
            Ok(Some(group)) => Some(group),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(
                    "Failed to look up group with GID {}: {}\nIgnoring...",
                    gid,
                    e
                );
                None
            }
        })
        .collect::<Vec<LibcGroup>>();

    Ok(groups)
}

/// Check if the current executable is running in SUID/SGID mode
#[cfg(feature = "suid-sgid-mode")]
pub fn executing_in_suid_sgid_mode() -> anyhow::Result<bool> {
    let euid = nix::unistd::geteuid();
    let uid = nix::unistd::getuid();
    let egid = nix::unistd::getegid();
    let gid = nix::unistd::getgid();

    Ok(euid != uid || egid != gid)
}

#[cfg(not(feature = "suid-sgid-mode"))]
#[inline]
pub fn executing_in_suid_sgid_mode() -> anyhow::Result<bool> {
    Ok(false)
}

impl UnixUser {
    pub fn from_uid(uid: u32) -> anyhow::Result<Self> {
        let libc_uid = nix::unistd::Uid::from_raw(uid);
        let libc_user = LibcUser::from_uid(libc_uid)
            .context("Failed to look up your UNIX username")?
            .ok_or(anyhow::anyhow!("Failed to look up your UNIX username"))?;

        let groups = get_unix_groups(&libc_user)?;

        Ok(UnixUser {
            username: libc_user.name,
            groups: groups.iter().map(|g| g.name.clone()).collect(),
        })
    }

    // pub fn from_enviroment() -> anyhow::Result<Self> {
    //     let libc_uid = nix::unistd::getuid();
    //     UnixUser::from_uid(libc_uid.as_raw())
    // }
}

#[inline]
pub(crate) fn yn(b: bool) -> &'static str {
    if b { "Y" } else { "N" }
}

#[inline]
pub(crate) fn rev_yn(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "y" => Some(true),
        "n" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_yn() {
        assert_eq!(yn(true), "Y");
        assert_eq!(yn(false), "N");
    }

    #[test]
    fn test_rev_yn() {
        assert_eq!(rev_yn("Y"), Some(true));
        assert_eq!(rev_yn("y"), Some(true));
        assert_eq!(rev_yn("N"), Some(false));
        assert_eq!(rev_yn("n"), Some(false));
        assert_eq!(rev_yn("X"), None);
    }
}
