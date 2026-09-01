use super::{run_capture, PackageInfo, PackageManager, PmCommand, RepoInfo};

pub struct Zypper;

fn split_pipes(line: &str) -> Vec<String> {
    line.split('|').map(|c| c.trim().to_string()).collect()
}

impl PackageManager for Zypper {
    fn display_name(&self) -> &'static str {
        "Zypper (openSUSE)"
    }

    fn binary(&self) -> &'static str {
        "zypper"
    }

    fn search(&self, query: &str) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("zypper", &["--non-interactive", "se", query])?;
        let mut results = Vec::new();
        for line in out.lines() {
            if !line.contains('|') || line.trim_start().starts_with('-') {
                continue;
            }
            let cols = split_pipes(line);
            if cols.len() < 3 || cols[1].eq_ignore_ascii_case("Name") {
                continue;
            }
            let status = cols[0].trim();
            results.push(PackageInfo {
                name: cols[1].clone(),
                version: String::new(),
                description: cols.get(2).cloned().unwrap_or_default(),
                installed: status.contains('i'),
            });
        }
        Ok(results)
    }

    fn list_installed(&self) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("rpm", &["-qa", "--qf", "%{NAME}\t%{VERSION}\t%{SUMMARY}\n"])?;
        let mut results = Vec::new();
        for line in out.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if let Some(name) = cols.first() {
                results.push(PackageInfo {
                    name: name.to_string(),
                    version: cols.get(1).unwrap_or(&"").to_string(),
                    description: cols.get(2).unwrap_or(&"").to_string(),
                    installed: true,
                });
            }
        }
        Ok(results)
    }

    fn install_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new("zypper", &["--non-interactive", "install", pkg], true)
    }

    fn remove_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new("zypper", &["--non-interactive", "remove", pkg], true)
    }

    fn update_index_cmd(&self) -> PmCommand {
        PmCommand::new("zypper", &["--non-interactive", "refresh"], true)
    }

    fn upgrade_all_cmd(&self) -> PmCommand {
        PmCommand::new("zypper", &["--non-interactive", "update"], true)
    }

    fn list_repos(&self) -> Result<Vec<RepoInfo>, String> {
        let out = run_capture("zypper", &["--non-interactive", "lr", "-d"])?;
        let mut repos = Vec::new();
        for line in out.lines() {
            if !line.contains('|') || line.trim_start().starts_with('-') {
                continue;
            }
            let cols = split_pipes(line);
            if cols.len() < 4 || cols[1].eq_ignore_ascii_case("Alias") {
                continue;
            }
            repos.push(RepoInfo {
                id: cols[1].clone(),
                name: cols.get(2).cloned().unwrap_or_default(),
                url: cols.last().cloned().unwrap_or_default(),
                enabled: cols
                    .get(3)
                    .map(|s| s.eq_ignore_ascii_case("Yes"))
                    .unwrap_or(true),
            });
        }
        Ok(repos)
    }

    fn add_repo_cmd(&self, repo: &str) -> PmCommand {
        // `repo` is a URL; zypper derives an alias automatically.
        PmCommand::new("zypper", &["ar", "-f", repo], true)
    }

    fn remove_repo_cmd(&self, repo_id: &str) -> PmCommand {
        PmCommand::new("zypper", &["rr", repo_id], true)
    }
}
