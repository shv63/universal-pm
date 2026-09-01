use super::{run_capture, PackageInfo, PackageManager, PmCommand, RepoInfo};
use std::collections::HashSet;
use std::fs;

pub struct Apt;

fn installed_set() -> HashSet<String> {
    run_capture("dpkg-query", &["-W", "-f=${Package}\t${Status}\n"])
        .map(|out| {
            out.lines()
                .filter_map(|l| {
                    let mut parts = l.split('\t');
                    let name = parts.next()?;
                    let status = parts.next()?;
                    if status.contains("install ok installed") {
                        Some(name.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

impl PackageManager for Apt {
    fn display_name(&self) -> &'static str {
        "APT (Debian/Ubuntu)"
    }

    fn binary(&self) -> &'static str {
        "apt"
    }

    fn search(&self, query: &str, _system: bool) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("apt-cache", &["search", query])?;
        let installed = installed_set();
        let mut results = Vec::new();
        for line in out.lines() {
            if let Some((name, desc)) = line.split_once(" - ") {
                let name = name.trim().to_string();
                results.push(PackageInfo {
                    installed: installed.contains(&name),
                    name,
                    version: String::new(),
                    description: desc.trim().to_string(),
                });
            }
        }
        Ok(results)
    }

    fn list_installed(&self, _system: bool) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture(
            "dpkg-query",
            &["-W", "-f=${Package}\t${Version}\t${Status}\t${binary:Summary}\n"],
        )?;
        let mut results = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 && parts[2].contains("install ok installed") {
                results.push(PackageInfo {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    description: parts.get(3).unwrap_or(&"").to_string(),
                    installed: true,
                });
            }
        }
        Ok(results)
    }

    fn install_cmd(&self, pkg: &str, _system: bool) -> PmCommand {
        PmCommand::new("apt-get", &["install", "-y", pkg], true)
    }

    fn remove_cmd(&self, pkg: &str, _system: bool) -> PmCommand {
        PmCommand::new("apt-get", &["remove", "-y", pkg], true)
    }

    fn update_index_cmd(&self, _system: bool) -> PmCommand {
        PmCommand::new("apt-get", &["update"], true)
    }

    fn upgrade_all_cmd(&self, _system: bool) -> PmCommand {
        PmCommand::new("apt-get", &["upgrade", "-y"], true)
    }

    fn list_repos(&self, _system: bool) -> Result<Vec<RepoInfo>, String> {
        let mut files = vec!["/etc/apt/sources.list".to_string()];
        if let Ok(entries) = fs::read_dir("/etc/apt/sources.list.d") {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "list").unwrap_or(false) {
                    files.push(p.to_string_lossy().to_string());
                }
            }
        }

        let mut repos = Vec::new();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            for (i, raw) in content.lines().enumerate() {
                let line = raw.trim();
                if line.is_empty() {
                    continue;
                }
                let enabled = !line.starts_with('#');
                let body = line.trim_start_matches('#').trim();
                if body.starts_with("deb ") || body.starts_with("deb-src ") {
                    repos.push(RepoInfo {
                        id: format!("{file}:{i}"),
                        name: file.rsplit('/').next().unwrap_or(&file).to_string(),
                        url: body.to_string(),
                        enabled,
                    });
                }
            }
        }
        Ok(repos)
    }

    fn add_repo_cmd(&self, repo: &str, _system: bool) -> PmCommand {
        // Expects a PPA-style ("ppa:user/name") or full `deb ...` line, both
        // of which `add-apt-repository` understands.
        PmCommand::new("add-apt-repository", &["-y", repo], true)
    }

    fn remove_repo_cmd(&self, repo_id: &str, _system: bool) -> PmCommand {
        // repo_id here is expected to be the original repo string (a
        // "ppa:user/name" or `deb ...` line) as shown in the UI, since
        // add-apt-repository --remove needs the same spec that added it.
        PmCommand::new("add-apt-repository", &["-y", "--remove", repo_id], true)
    }
}
