pub mod apk;
pub mod apt;
pub mod dnf;
pub mod flatpak;
pub mod pacman;
pub mod zypper;

use std::process::Command;

/// Minimal stand-in for the `which` crate: true if `bin` is an executable
/// file somewhere on `$PATH` (or is itself a path to one).
pub fn which(bin: &str) -> bool {
    if bin.contains('/') {
        return std::path::Path::new(bin).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
        })
        .unwrap_or(false)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub installed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoInfo {
    /// Short identifier used to enable/disable/remove the repo.
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

/// A command to run, split into the privileged launcher (pkexec/sudo) and
/// the actual program + args, so the UI layer can decide how to elevate.
#[derive(Clone, Debug)]
pub struct PmCommand {
    pub program: String,
    pub args: Vec<String>,
    pub needs_root: bool,
}

impl PmCommand {
    pub fn new(program: &str, args: &[&str], needs_root: bool) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            needs_root,
        }
    }
}

/// Common interface every distro package manager backend implements.
/// UI code never talks to `apt`/`dnf`/`pacman`/`zypper` directly - it
/// only ever calls through this trait, which is what makes the frontend
/// distro-agnostic.
pub trait PackageManager: Send + Sync {
    /// Human readable name, e.g. "APT (Debian/Ubuntu)".
    fn display_name(&self) -> &'static str;

    /// The underlying binary, e.g. "apt".
    fn binary(&self) -> &'static str;

    fn search(&self, query: &str) -> Result<Vec<PackageInfo>, String>;
    fn list_installed(&self) -> Result<Vec<PackageInfo>, String>;

    fn install_cmd(&self, pkg: &str) -> PmCommand;
    fn remove_cmd(&self, pkg: &str) -> PmCommand;
    fn update_index_cmd(&self) -> PmCommand;
    fn upgrade_all_cmd(&self) -> PmCommand;

    fn list_repos(&self) -> Result<Vec<RepoInfo>, String>;
    fn add_repo_cmd(&self, repo: &str) -> PmCommand;
    fn remove_repo_cmd(&self, repo_id: &str) -> PmCommand;
}

/// Runs a read-only (non-privileged) command and returns stdout as a String.
pub fn run_capture(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {program}: {e}"))?;
    if !out.status.success() {
        // Some tools (e.g. `dpkg -l`, `zypper lr`) still print useful data
        // on non-zero exit; fall back to stdout if present, else stderr.
        if !out.stdout.is_empty() {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Executes a `PmCommand`, transparently wrapping it in `pkexec` (falling
/// back to `sudo`) when root is required. Returns combined stdout+stderr.
pub fn execute(cmd: &PmCommand) -> Result<String, String> {
    let (launcher, mut full_args): (String, Vec<String>) = if cmd.needs_root {
        let launcher = if which("pkexec") {
            "pkexec".to_string()
        } else {
            "sudo".to_string()
        };
        let mut args = vec![cmd.program.clone()];
        args.extend(cmd.args.clone());
        (launcher, args)
    } else {
        (cmd.program.clone(), cmd.args.clone())
    };

    // Non-interactive where the backend supports it, so we never hang
    // waiting for a [Y/n] prompt on stdin the GUI can't answer.
    if !cmd.needs_root {
        full_args = cmd.args.clone();
    }

    let out = Command::new(&launcher)
        .args(&full_args)
        .output()
        .map_err(|e| format!("failed to run {launcher}: {e}"))?;

    let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));

    if out.status.success() {
        Ok(combined)
    } else {
        Err(if combined.trim().is_empty() {
            format!("{launcher} exited with status {}", out.status)
        } else {
            combined
        })
    }
}
