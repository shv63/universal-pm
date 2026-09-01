use super::{run_capture, PackageInfo, PackageManager, PmCommand, RepoInfo};

/// Flatpak is modeled as just another `PackageManager` backend: "packages"
/// are flatpak application IDs and "repos" are flatpak remotes (e.g.
/// Flathub). This lets the same UI tabs (Search/Install, Installed,
/// Repositories) drive it with no special-casing.
///
/// All operations use `--user` so no root/pkexec prompt is needed for the
/// common case; toggle `SYSTEM_WIDE` below if you'd rather manage
/// system-wide flatpaks (requires root).
pub struct Flatpak;

impl PackageManager for Flatpak {
    fn display_name(&self) -> &'static str {
        "Flatpak"
    }

    fn binary(&self) -> &'static str {
        "flatpak"
    }

    fn search(&self, query: &str, system: bool) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture(
            "flatpak",
            &["search", "--columns=name,description,application", query],
        )?;
        let installed = if system {
            // Try to get system list via execute (may require root); fall back to empty
            super::execute(&PmCommand::new(
                "flatpak",
                &["list", "--system", "--app", "--columns=application"],
                true,
            ))
            .unwrap_or_default()
        } else {
            run_capture(
                "flatpak",
                &["list", "--user", "--app", "--columns=application"],
            )
            .unwrap_or_default()
        };
        let mut results = Vec::new();
        for line in out.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 3 {
                continue;
            }
            let app_id = cols[2].trim().to_string();
            if app_id.is_empty() || app_id.eq_ignore_ascii_case("application id") {
                continue;
            }
            results.push(PackageInfo {
                name: app_id.clone(),
                version: cols[0].trim().to_string(), // display name, shown as "version" col in UI
                description: cols[1].trim().to_string(),
                installed: installed.lines().any(|l| l.trim() == app_id),
            });
        }
        Ok(results)
    }

    fn list_installed(&self, system: bool) -> Result<Vec<PackageInfo>, String> {
        if system {
            let out = super::execute(&PmCommand::new(
                "flatpak",
                &["list", "--system", "--app", "--columns=application,name,version"],
                true,
            ))?;
            let mut results = Vec::new();
            for line in out.lines() {
                let cols: Vec<&str> = line.split('\t').collect();
                if cols.is_empty() || cols[0].trim().is_empty() {
                    continue;
                }
                results.push(PackageInfo {
                    name: cols[0].trim().to_string(),
                    version: cols.get(2).unwrap_or(&"").trim().to_string(),
                    description: cols.get(1).unwrap_or(&"").trim().to_string(),
                    installed: true,
                });
            }
            Ok(results)
        } else {
            let out = run_capture(
                "flatpak",
                &[
                    "list",
                    "--user",
                    "--app",
                    "--columns=application,name,version",
                ],
            )?;
            let mut results = Vec::new();
            for line in out.lines() {
                let cols: Vec<&str> = line.split('\t').collect();
                if cols.is_empty() || cols[0].trim().is_empty() {
                    continue;
                }
                results.push(PackageInfo {
                    name: cols[0].trim().to_string(),
                    version: cols.get(2).unwrap_or(&"").trim().to_string(),
                    description: cols.get(1).unwrap_or(&"").trim().to_string(),
                    installed: true,
                });
            }
            Ok(results)
        }
    }

    fn install_cmd(&self, pkg: &str, system: bool) -> PmCommand {
        PmCommand::new(
            "flatpak",
            &["install", if system { "--system" } else { "--user" }, "-y", "--or-update", pkg],
            system,
        )
    }

    fn remove_cmd(&self, pkg: &str, system: bool) -> PmCommand {
        PmCommand::new("flatpak", &["uninstall", if system { "--system" } else { "--user" }, "-y", pkg], system)
    }

    fn update_index_cmd(&self, system: bool) -> PmCommand {
        PmCommand::new("flatpak", &["update", if system { "--system" } else { "--user" }, "--appstream", "-y"], system)
    }

    fn upgrade_all_cmd(&self, system: bool) -> PmCommand {
        PmCommand::new("flatpak", &["update", if system { "--system" } else { "--user" }, "-y"], system)
    }

    fn list_repos(&self, system: bool) -> Result<Vec<RepoInfo>, String> {
        let out = if system {
            super::execute(&PmCommand::new("flatpak", &["remotes", "--system", "--columns=name,url,title,disabled"], true))?
        } else {
            run_capture(
                "flatpak",
                &["remotes", "--user", "--columns=name,url,title,disabled"],
            )?
        };
        let mut repos = Vec::new();
        for line in out.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.is_empty() || cols[0].trim().is_empty() {
                continue;
            }
            repos.push(RepoInfo {
                id: cols[0].trim().to_string(),
                name: cols.get(2).unwrap_or(&cols[0]).trim().to_string(),
                url: cols.get(1).unwrap_or(&"\"").trim().to_string(),
                enabled: cols
                    .get(3)
                    .map(|s| !s.trim().eq_ignore_ascii_case("true"))
                    .unwrap_or(true),
            });
        }
        Ok(repos)
    }

    fn add_repo_cmd(&self, repo: &str, system: bool) -> PmCommand {
        // Expects "<name> <url>", e.g. "flathub https://flathub.org/repo/flathub.flatpakrepo"
        let mut parts = repo.split_whitespace();
        let name = parts.next().unwrap_or("remote");
        let url = parts.next().unwrap_or(repo);
        PmCommand::new(
            "flatpak",
            &[
                "remote-add",
                if system { "--system" } else { "--user" },
                "--if-not-exists",
                name,
                url,
            ],
            system,
        )
    }

    fn remove_repo_cmd(&self, repo_id: &str, system: bool) -> PmCommand {
        PmCommand::new("flatpak", &["remote-delete", if system { "--system" } else { "--user" }, repo_id], system)
    }
}

