use super::pacman::Pacman;
use super::{run_capture, PackageInfo, PackageManager, PmCommand, RepoInfo};

/// `yay` is a pacman wrapper that also searches/builds AUR packages. It
/// speaks the same `-S`/`-R`/`-Ss`/`-Q`/`-Sy`/`-Syu` flags as pacman, so
/// this backend mostly just swaps the binary name for search/install/
/// remove/update/upgrade. Repositories are still `pacman.conf` sections
/// (yay doesn't have its own repo concept), so `list_repos`/`add_repo_cmd`/
/// `remove_repo_cmd` delegate straight to `Pacman`.
///
/// IMPORTANT CAVEAT: AUR packages are built with `makepkg`, which refuses
/// to run as root. That means yay operations must NOT be launched via
/// `pkexec`/`sudo` wrapping the whole command (unlike every other backend
/// here) — yay calls `sudo` itself, internally, only for the final
/// pacman install step. Since this GUI runs commands non-interactively
/// (no attached terminal for a password prompt), a plain `yay -S <pkg>`
/// will hang or fail unless you've set up passwordless sudo for pacman,
/// e.g. via `sudo visudo` adding:
///   `yourusername ALL=(ALL) NOPASSWD: /usr/bin/pacman`
/// Without that, use the plain Pacman backend for anything needing root,
/// and reserve yay for searching/inspecting AUR packages.
pub struct Yay;

fn strip_repo_prefix(header: &str) -> &str {
    header.splitn(2, '/').nth(1).unwrap_or(header)
}

impl PackageManager for Yay {
    fn display_name(&self) -> &'static str {
        "Yay (Arch + AUR)"
    }

    fn binary(&self) -> &'static str {
        "yay"
    }

    fn search(&self, query: &str, _system: bool) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("yay", &["-Ss", query])?;
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
            let body = strip_repo_prefix(header);
            let mut cols = body.split_whitespace();
            let name = cols.next().unwrap_or("").to_string();
            let version = cols.next().unwrap_or("").to_string();
            let desc = lines
                .get(i + 1)
                .filter(|l| l.starts_with(' '))
                .map(|l| l.trim().to_string())
                .unwrap_or_default();
            if !name.is_empty() {
                let already_installed = header.contains("[installed")
                    || installed.lines().any(|l| l.starts_with(&format!("{name} ")));
                results.push(PackageInfo {
                    name,
                    version,
                    description: desc,
                    installed: already_installed,
                });
            }
            i += 2;
        }
        Ok(results)
    }

    fn list_installed(&self, _system: bool) -> Result<Vec<PackageInfo>, String> {
        // AUR packages installed via yay still show up in plain `pacman -Q`.
        Pacman.list_installed(false)
    }

    fn install_cmd(&self, pkg: &str, _system: bool) -> PmCommand {
        // needs_root: false on purpose — see the module-level caveat above.
        PmCommand::new("yay", &["-S", "--noconfirm", pkg], false)
    }

    fn remove_cmd(&self, pkg: &str, _system: bool) -> PmCommand {
        PmCommand::new("yay", &["-R", "--noconfirm", pkg], false)
    }

    fn update_index_cmd(&self, _system: bool) -> PmCommand {
        PmCommand::new("yay", &["-Sy"], false)
    }

    fn upgrade_all_cmd(&self, _system: bool) -> PmCommand {
        PmCommand::new("yay", &["-Syu", "--noconfirm"], false)
    }

    fn list_repos(&self, _system: bool) -> Result<Vec<RepoInfo>, String> {
        Pacman.list_repos(false)
    }

    fn add_repo_cmd(&self, repo: &str, _system: bool) -> PmCommand {
        Pacman.add_repo_cmd(repo, false)
    }

    fn remove_repo_cmd(&self, repo_id: &str, _system: bool) -> PmCommand {
        Pacman.remove_repo_cmd(repo_id, false)
    }
}
