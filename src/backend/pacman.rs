use super::{run_capture, PackageInfo, PackageManager, PmCommand, RepoInfo};
use std::fs;

pub struct Pacman;

impl PackageManager for Pacman {
    fn display_name(&self) -> &'static str {
        "Pacman (Arch Linux)"
    }

    fn binary(&self) -> &'static str {
        "pacman"
    }

    fn search(&self, query: &str, _system: bool) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("pacman", &["-Ss", query])?;
        let installed = run_capture("pacman", &["-Q"]).unwrap_or_default();
        let mut results = Vec::new();
        let lines: Vec<&str> = out.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let header = lines[i];
            if header.starts_with(' ') {
                i += 1;
                continue;
            }
            // "repo/name  version  [installed]"
            let after_slash = header.splitn(2, '/').nth(1).unwrap_or(header);
            let mut cols = after_slash.split_whitespace();
            let name = cols.next().unwrap_or("").to_string();
            let version = cols.next().unwrap_or("").to_string();
            let desc = lines
                .get(i + 1)
                .filter(|l| l.starts_with(' '))
                .map(|l| l.trim().to_string())
                .unwrap_or_default();
            if !name.is_empty() {
                results.push(PackageInfo {
                    installed: installed.lines().any(|l| l.starts_with(&format!("{name} "))),
                    name,
                    version,
                    description: desc,
                });
            }
            i += 2;
        }
        Ok(results)
    }

    fn list_installed(&self, _system: bool) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("pacman", &["-Q"])?;
        let mut results = Vec::new();
        for line in out.lines() {
            let mut cols = line.split_whitespace();
            if let (Some(name), Some(version)) = (cols.next(), cols.next()) {
                results.push(PackageInfo {
                    name: name.to_string(),
                    version: version.to_string(),
                    description: String::new(),
                    installed: true,
                });
            }
        }
        Ok(results)
    }

    fn install_cmd(&self, pkg: &str, _system: bool) -> PmCommand {
        PmCommand::new("pacman", &["-S", "--noconfirm", pkg], true)
    }

    fn remove_cmd(&self, pkg: &str, _system: bool) -> PmCommand {
        PmCommand::new("pacman", &["-R", "--noconfirm", pkg], true)
    }

    fn update_index_cmd(&self, _system: bool) -> PmCommand {
        PmCommand::new("pacman", &["-Sy"], true)
    }

    fn upgrade_all_cmd(&self, _system: bool) -> PmCommand {
        PmCommand::new("pacman", &["-Syu", "--noconfirm"], true)
    }

    fn list_repos(&self, _system: bool) -> Result<Vec<RepoInfo>, String> {
        let content = fs::read_to_string("/etc/pacman.conf").unwrap_or_default();
        let mut repos = Vec::new();
        let mut current: Option<String> = None;
        let mut server = String::new();
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                if let Some(name) = current.take() {
                    if name != "options" {
                        repos.push(RepoInfo {
                            id: name.clone(),
                            name,
                            url: server.clone(),
                            enabled: true,
                        });
                    }
                }
                current = Some(rest.to_string());
                server.clear();
            } else if let Some(v) = line.strip_prefix("Server") {
                server = v.trim_start_matches('=').trim().to_string();
            } else if let Some(v) = line.strip_prefix("Include") {
                if server.is_empty() {
                    server = format!("(via {})", v.trim_start_matches('=').trim());
                }
            }
        }
        if let Some(name) = current {
            if name != "options" {
                repos.push(RepoInfo {
                    id: name.clone(),
                    name,
                    url: server,
                    enabled: true,
                });
            }
        }
        Ok(repos)
    }

    fn add_repo_cmd(&self, repo: &str, _system: bool) -> PmCommand {
        // `repo` is expected to already be a full block, e.g.:
        //   [myrepo]\nServer = https://example.com/$repo/$arch
        // appended verbatim to pacman.conf.
        PmCommand::new(
            "sh",
            &["-c", &format!("printf '%s\\n' {} >> /etc/pacman.conf", shell_quote(repo))],
            true,
        )
    }

    fn remove_repo_cmd(&self, repo_id: &str, _system: bool) -> PmCommand {
        // Strips the [repo_id] section (and its body, up to the next
        // section or EOF) out of pacman.conf using awk.
        let script = format!(
            "awk -v RS= -v ORS=\"\\n\\n\" '!/^\\[{repo_id}\\]/' /etc/pacman.conf > /tmp/pacman.conf.new && mv /tmp/pacman.conf.new /etc/pacman.conf"
        );
        PmCommand::new("sh", &["-c", &script], true)
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
