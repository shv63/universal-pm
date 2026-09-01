pub mod apk;
pub mod apt;
pub mod dnf;
pub mod flatpak;
pub mod pacman;
pub mod yay;
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
    /// Optional password to send to sudo via stdin (used when the UI
    /// collected a password from the user). If None the executor will
    /// prefer pkexec or sudo without stdin.
    pub password: Option<String>,
}

impl PmCommand {
    pub fn new(program: &str, args: &[&str], needs_root: bool) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            needs_root,
            password: None,
        }
    }

    pub fn with_password(mut self, pw: String) -> Self {
        self.password = Some(pw);
        self
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

    /// Search for packages. `system` is ignored by most backends but used
    /// by Flatpak to select a scope when relevant.
    fn search(&self, query: &str, system: bool) -> Result<Vec<PackageInfo>, String>;

    /// List installed packages. `system` chooses system/user scope for
    /// backends that support it (flatpak); ignored elsewhere.
    fn list_installed(&self, system: bool) -> Result<Vec<PackageInfo>, String>;

    /// Build an install command for `pkg`. `system` allows backends like
    /// Flatpak to return system-wide install commands when requested.
    fn install_cmd(&self, pkg: &str, system: bool) -> PmCommand;
    fn remove_cmd(&self, pkg: &str, system: bool) -> PmCommand;
    fn update_index_cmd(&self, system: bool) -> PmCommand;
    fn upgrade_all_cmd(&self, system: bool) -> PmCommand;

    fn list_repos(&self, system: bool) -> Result<Vec<RepoInfo>, String>;
    fn add_repo_cmd(&self, repo: &str, system: bool) -> PmCommand;
    fn remove_repo_cmd(&self, repo_id: &str, system: bool) -> PmCommand;
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
use zeroize::Zeroize;

pub fn execute(cmd: &PmCommand) -> Result<String, String> {
    let (launcher, mut full_args, pass_via_stdin): (String, Vec<String>, bool) = if cmd.needs_root {
        // If a password was supplied use "sudo -S" and feed the password to stdin.
        if cmd.password.is_some() {
            let mut args = vec![cmd.program.clone()];
            args.extend(cmd.args.clone());
            ("sudo".to_string(), args, true)
        } else if which("pkexec") {
            let mut args = vec![cmd.program.clone()];
            args.extend(cmd.args.clone());
            ("pkexec".to_string(), args, false)
        } else {
            let mut args = vec![cmd.program.clone()];
            args.extend(cmd.args.clone());
            ("sudo".to_string(), args, false)
        }
    } else {
        (cmd.program.clone(), cmd.args.clone(), false)
    };

    // Non-interactive where the backend supports it, so we never hang
    // waiting for a [Y/n] prompt on stdin the GUI can't answer.
    if !cmd.needs_root {
        full_args = cmd.args.clone();
    }

    let mut child = Command::new(&launcher)
        .args(&full_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {launcher}: {e}"))?;

    if pass_via_stdin {
        if let Some(mut stdin) = child.stdin.take() {
            if let Some(pw) = &cmd.password {
                use std::io::Write;
                // Clone into a local mutable string so we can zeroize it immediately
                // after writing it to the child's stdin. This avoids leaving the
                // plaintext lingering in this stack frame. The original
                // PmCommand still owns its copy; the UI will zero that separately.
                let mut local_pw = pw.clone();
                let _ = stdin.write_all(format!("{}\n", local_pw).as_bytes());
                local_pw.zeroize();
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for {launcher}: {e}"))?;

    let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    if output.status.success() {
        Ok(combined)
    } else {
        Err(if combined.trim().is_empty() {
            format!("{launcher} exited with status {}", output.status)
        } else {
            combined
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_capture_echo() {
        let out = run_capture("echo", &["hello"]).expect("echo should run");
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn execute_echo() {
        let cmd = PmCommand::new("echo", &["world"], false);
        let out = execute(&cmd).expect("execute should run echo");
        assert!(out.contains("world"));
    }
}
