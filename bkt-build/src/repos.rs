use anyhow::{anyhow, bail, Context, Result};
use bkt_common::manifest::ExternalReposManifest;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn setup_repos(manifest_path: &Path) -> Result<()> {
    let manifest = ExternalReposManifest::load_from(manifest_path)
        .context("failed to load external repos manifest")?;

    if manifest.repos.is_empty() {
        bail!(
            "no external repos found in manifest {}",
            manifest_path.display()
        );
    }

    std::fs::create_dir_all("/etc/yum.repos.d").context("failed to create /etc/yum.repos.d")?;

    for repo in &manifest.repos {
        run_command(
            "rpm",
            vec!["--import".to_string(), repo.gpg_key.clone()],
            &format!("failed to import GPG key for repo '{}'", repo.name),
        )?;

        let repo_file = repo_file_path(&repo.name)?;
        let content = format!(
            "[{name}]\nname={display_name}\nbaseurl={baseurl}\nenabled=1\ngpgcheck=1\nrepo_gpgcheck=0\ngpgkey={gpg_key}\n",
            name = repo.name,
            display_name = repo.display_name,
            baseurl = repo.baseurl,
            gpg_key = repo.gpg_key
        );
        std::fs::write(&repo_file, content)
            .with_context(|| format!("failed to write {}", repo_file.display()))?;
        eprintln!("Wrote {}", repo_file.display());
    }

    Ok(())
}

pub fn download_rpms(repo_name: &str, manifest_path: &Path) -> Result<()> {
    let manifest = ExternalReposManifest::load_from(manifest_path)
        .context("failed to load external repos manifest")?;

    let repo = manifest
        .find(repo_name)
        .ok_or_else(|| anyhow!("repo '{}' not found in manifest", repo_name))?;

    if repo.packages.is_empty() {
        bail!("repo '{}' has no packages configured", repo_name);
    }

    std::fs::create_dir_all("/rpms").context("failed to create /rpms")?;

    let mut args = vec![
        "download".to_string(),
        "--destdir".to_string(),
        "/rpms".to_string(),
        "--disablerepo=*".to_string(),
        format!("--enablerepo={}", repo.name),
    ];
    args.extend(repo.packages.iter().cloned());

    run_command(
        "dnf",
        args,
        &format!("failed to download RPMs for repo '{}'", repo.name),
    )?;

    Ok(())
}

fn run_command(program: &str, args: Vec<String>, error_context: &str) -> Result<()> {
    let status = Command::new(program)
        .args(args.iter().map(String::as_str))
        .status()
        .with_context(|| format!("{}: could not execute {}", error_context, program))?;

    if !status.success() {
        bail!(
            "{}: {} exited with status {}",
            error_context,
            program,
            status
        );
    }

    Ok(())
}

fn repo_file_path(name: &str) -> Result<PathBuf> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!(
            "invalid repo name '{}': only [A-Za-z0-9_-] are allowed",
            name
        );
    }

    Ok(Path::new("/etc/yum.repos.d").join(format!("{}.repo", name)))
}
