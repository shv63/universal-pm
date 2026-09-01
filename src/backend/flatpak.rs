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

const SYSTEM_WIDE: bool = false;

fn scope_flag() -> &'static str {
    if SYSTEM_WIDE {
        "--system"
    } else {
        "--user"
    }
}

impl PackageManager for Flatpak {
    fn display_name(&self) -> &'static str {
        "Flatpak"
    }

    fn binary(&self) -> &'static str {
        "flatpak"
    }

    fn search(&self, query: &str) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture(
            "flatpak",
            &["search", "--columns=name,description,application", query],
        )?;
        let installed = run_capture(
            "flatpak",
            &["list", scope_flag(), "--app", "--columns=application"],
        )
        .unwrap_or_default();
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

    fn list_installed(&self) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture(
            "flatpak",
            &[
                "list",
                scope_flag(),
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

    fn install_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new(
            "flatpak",
            &["install", scope_flag(), "-y", "--or-update", pkg],
            false,
        )
    }

    fn remove_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new("flatpak", &["uninstall", scope_flag(), "-y", pkg], false)
    }

    fn update_index_cmd(&self) -> PmCommand {
        PmCommand::new("flatpak", &["update", scope_flag(), "--appstream", "-y"], false)
    }

    fn upgrade_all_cmd(&self) -> PmCommand {
        PmCommand::new("flatpak", &["update", scope_flag(), "-y"], false)
    }

    fn list_repos(&self) -> Result<Vec<RepoInfo>, String> {
        let out = run_capture(
            "flatpak",
            &["remotes", scope_flag(), "--columns=name,url,title,disabled"],
        )?;
        let mut repos = Vec::new();
        for line in out.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.is_empty() || cols[0].trim().is_empty() {
                continue;
            }
            repos.push(RepoInfo {
                id: cols[0].trim().to_string(),
                name: cols.get(2).unwrap_or(&cols[0]).trim().to_string(),
                url: cols.get(1).unwrap_or(&"").trim().to_string(),
                enabled: cols
                    .get(3)
                    .map(|s| !s.trim().eq_ignore_ascii_case("true"))
                    .unwrap_or(true),
            });
        }
        Ok(repos)
    }

    fn add_repo_cmd(&self, repo: &str) -> PmCommand {
        // Expects "<name> <url>", e.g. "flathub https://flathub.org/repo/flathub.flatpakrepo"
        let mut parts = repo.split_whitespace();
        let name = parts.next().unwrap_or("remote");
        let url = parts.next().unwrap_or(repo);
        PmCommand::new(
            "flatpak",
            &[
                "remote-add",
                scope_flag(),
                "--if-not-exists",
                name,
                url,
            ],
            false,
        )
    }

    fn remove_repo_cmd(&self, repo_id: &str) -> PmCommand {
        PmCommand::new("flatpak", &["remote-delete", scope_flag(), repo_id], false)
    }
}
