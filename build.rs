#[cfg(feature = "mysql-admutils-compatibility")]
use anyhow::anyhow;
#[cfg(feature = "mysql-admutils-compatibility")]
use std::{env, os::unix::fs::symlink, path::PathBuf};

fn get_git_commit() -> Option<String> {
    let repo = git2::Repository::discover(".").ok()?;
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    Some(commit.id().to_string())
}

fn get_git_commit_date() -> Option<String> {
    let repo = git2::Repository::discover(".").ok()?;
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    format_git_time(commit.time())
}

fn format_git_time(git_time: git2::Time) -> Option<String> {
    let offset = time::UtcOffset::from_whole_seconds(git_time.offset_minutes() * 60).ok()?;
    let datetime = time::OffsetDateTime::from_unix_timestamp(git_time.seconds())
        .ok()?
        .to_offset(offset);
    let format = time::macros::format_description!("[year]-[month]-[day]");
    datetime.format(&format).ok()
}

fn get_git_dirty() -> Option<bool> {
    let repo = git2::Repository::discover(".").ok()?;
    let mut status_options = git2::StatusOptions::new();
    status_options
        .include_untracked(true)
        .include_ignored(false);
    let statuses = repo.statuses(Some(&mut status_options)).ok()?;
    Some(!statuses.is_empty())
}

fn embed_build_time_info() {
    let commit = option_env!("GIT_COMMIT")
        .map(|s| s.to_string())
        .or_else(get_git_commit)
        .unwrap_or_else(|| "unknown".to_string());

    let commit_date = option_env!("GIT_COMMIT_DATE")
        .map(|s| s.to_string())
        .or_else(get_git_commit_date)
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = option_env!("GIT_DIRTY")
        .map(|s| s == "true")
        .or_else(get_git_dirty)
        .unwrap_or(false);

    let build_profile = std::env::var("OUT_DIR")
        .unwrap_or_else(|_| "unknown".to_string())
        .split(std::path::MAIN_SEPARATOR)
        .nth_back(3)
        .unwrap_or("unknown")
        .to_string();

    let dependencies = build_info_build::build_script()
        .collect_runtime_dependencies(build_info_build::DependencyDepth::Depth(1))
        .build()
        .crate_info
        .dependencies
        .into_iter()
        .map(|dep| format!("{}: {}", dep.name, dep.version))
        .collect::<Vec<_>>()
        .join(";");

    println!("cargo:rustc-env=GIT_COMMIT={}", commit);
    println!("cargo:rustc-env=GIT_COMMIT_DATE={}", commit_date);
    println!("cargo:rustc-env=GIT_DIRTY={}", dirty);
    println!("cargo:rustc-env=BUILD_PROFILE={}", build_profile);
    println!("cargo:rustc-env=DEPENDENCY_LIST={}", dependencies);
}

fn generate_mysql_admutils_symlinks() -> anyhow::Result<()> {
    // NOTE: This is slightly illegal, and depends on implementation details.
    //       But it is only here for ease of testing the compatibility layer,
    //       and not critical in any way. Considering the code is never going
    //       to be used as a library, it should be fine.
    let target_profile_dir: PathBuf = PathBuf::from(env::var("OUT_DIR")?)
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or(anyhow!("Could not resolve target profile directory"))?
        .to_path_buf();

    if !target_profile_dir.exists() {
        std::fs::create_dir_all(&target_profile_dir)?;
    }

    if !target_profile_dir.join("mysql-useradm").exists() {
        symlink(
            PathBuf::from("./muscl"),
            target_profile_dir.join("mysql-useradm"),
        )
        .ok();
    }

    if !target_profile_dir.join("mysql-dbadm").exists() {
        symlink(
            PathBuf::from("./muscl"),
            target_profile_dir.join("mysql-dbadm"),
        )
        .ok();
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "mysql-admutils-compatibility")]
    generate_mysql_admutils_symlinks()?;

    embed_build_time_info();

    Ok(())
}
