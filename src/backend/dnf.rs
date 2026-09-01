use super::{run_capture, PackageInfo, PackageManager, PmCommand, RepoInfo};

pub struct Dnf;

impl PackageManager for Dnf {
    fn display_name(&self) -> &'static str {
        "DNF (Fedora/RHEL)"
    }

    fn binary(&self) -> &'static str {
        "dnf"
    }

    fn search(&self, query: &str) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("dnf", &["-q", "search", query])?;
        let installed = run_capture("dnf", &["-q", "list", "installed"]).unwrap_or_default();
        let mut results = Vec::new();
        for line in out.lines() {
            let Some((name_arch, desc)) = line.split_once(" : ") else {
                continue;
            };
            let name = name_arch.split('.').next().unwrap_or(name_arch).trim();
            if name.is_empty() {
                continue;
            }
            results.push(PackageInfo {
                name: name.to_string(),
                version: String::new(),
                description: desc.trim().to_string(),
                installed: installed.lines().any(|l| l.starts_with(name)),
            });
        }
        Ok(results)
    }

    fn list_installed(&self) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("dnf", &["-q", "list", "installed"])?;
        let mut results = Vec::new();
        for line in out.lines() {
            if line.starts_with("Installed Packages") || line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 2 {
                let name = cols[0].split('.').next().unwrap_or(cols[0]);
                results.push(PackageInfo {
                    name: name.to_string(),
                    version: cols[1].to_string(),
                    description: cols.get(2).unwrap_or(&"").to_string(),
                    installed: true,
                });
            }
        }
        Ok(results)
    }

    fn install_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new("dnf", &["install", "-y", pkg], true)
    }

    fn remove_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new("dnf", &["remove", "-y", pkg], true)
    }

    fn update_index_cmd(&self) -> PmCommand {
        PmCommand::new("dnf", &["makecache"], true)
    }

    fn upgrade_all_cmd(&self) -> PmCommand {
        PmCommand::new("dnf", &["upgrade", "-y"], true)
    }

    fn list_repos(&self) -> Result<Vec<RepoInfo>, String> {
        let out = run_capture("dnf", &["-q", "repolist", "--all"])?;
        let mut repos = Vec::new();
        for line in out.lines() {
            if line.starts_with("repo id") || line.trim().is_empty() {
                continue;
            }
            // Format: "<id>   <name>   <status>", columns are whitespace
            // padded so we split on 2+ spaces to keep names with spaces.
            let cols: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if cols.is_empty() {
                continue;
            }
            let id = cols[0].trim().to_string();
            let rest = cols.get(1).unwrap_or(&"").trim();
            let enabled = !rest.to_lowercase().ends_with("disabled");
            repos.push(RepoInfo {
                id: id.clone(),
                name: rest.trim_end_matches("disabled").trim_end_matches("enabled").trim().to_string(),
                url: id,
                enabled,
            });
        }
        Ok(repos)
    }

    fn add_repo_cmd(&self, repo: &str) -> PmCommand {
        PmCommand::new("dnf", &["config-manager", "--add-repo", repo], true)
    }

    fn remove_repo_cmd(&self, repo_id: &str) -> PmCommand {
        // Disabling rather than deleting the .repo file: reversible, and
        // avoids guessing which file on disk owns this repo id.
        PmCommand::new(
            "dnf",
            &["config-manager", "--set-disabled", repo_id],
            true,
        )
    }
}
