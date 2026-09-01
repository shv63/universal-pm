pub mod apk;
pub mod apt;
pub mod dnf;
pub mod flatpak;
pub mod pacman;
pub mod yay;
pub mod zypper;

use std::process::Command;
use std::os::unix::fs::OpenOptionsExt;

/// Minimal stand-in for the `which` crate: true if `bin` is an executable
/// file somewhere on `$PATH` (or is itself a path to one).
pub fn which(bin: &str) -> bool {
    which_path(bin).is_some()
}

/// Return the full path to `bin` if it exists on PATH (or is an absolute path).
pub fn which_path(bin: &str) -> Option<String> {
    let p = std::path::Path::new(bin);
    if p.is_absolute() || bin.contains('/') {
        if p.is_file() {
            return Some(bin.to_string());
        }
        return None;
    }
    match std::env::var_os("PATH") {
        Some(paths) => {
            for dir in std::env::split_paths(&paths) {
                let cand = dir.join(bin);
                if cand.is_file() {
                    if let Some(s) = cand.to_str() {
                        return Some(s.to_string());
                    }
                }
            }
            None
        }
        None => None,
    }
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
    #[allow(dead_code)]
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
            // Use `sudo -S` to allow feeding the password on stdin.
            let mut args = vec!["-S".to_string(), cmd.program.clone()];
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

    // If the attempt failed due to sudo requiring a TTY or password and we
    // have a password, try using SUDO_ASKPASS as a fallback so GUI-provided
    // passwords can be used on systems where sudo wants an interactive TTY.
    if !output.status.success() {
        let low = combined.to_lowercase();
        let tty_err = low.contains("terminal is required") || low.contains("a password is required") || low.contains("password is required");
        if pass_via_stdin && tty_err {
            if let Some(pw) = &cmd.password {
                // Create a small temporary askpass helper that prints the
                // password. Use a unique filename under /tmp and restrict perms.
                use std::io::Write;
                use std::time::{SystemTime, UNIX_EPOCH};
                let pid = std::process::id();
                let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
                let tmp_path = format!("/tmp/universal-pm-askpass-{}-{}.sh", pid, nanos);
                // Escape single quotes in the password for a single-quoted shell string.
                let mut local_pw = pw.clone();
                let esc_pw = local_pw.replace("'", r"'\''");
                // Write helper script
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).write(true).mode(0o700).open(&tmp_path) {
                    let _ = f.write_all(format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", esc_pw).as_bytes());
                    let _ = f.flush();
                }

                // Prefer an external askpass helper (if installed) so we don't
                // need to write a helper file. Common helpers include
                // ssh-askpass, x11-ssh-askpass, gnome-ssh-askpass, ksshaskpass,
                // qt5-askpass, openssh-askpass.
                let candidates = [
                    "ssh-askpass",
                    "x11-ssh-askpass",
                    "gnome-ssh-askpass",
                    "ksshaskpass",
                    "qt5-askpass",
                    "openssh-askpass",
                ];
                for cand in &candidates {
                    if let Some(path) = which_path(cand) {
                        let mut args = vec!["-A".to_string(), cmd.program.clone()];
                        args.extend(cmd.args.clone());
                        let ask_res = Command::new("sudo")
                            .args(&args)
                            .env("SUDO_ASKPASS", path)
                            .output();
                        match ask_res {
                            Ok(out) => {
                                let mut comb = String::from_utf8_lossy(&out.stdout).to_string();
                                comb.push_str(&String::from_utf8_lossy(&out.stderr));
                                if out.status.success() {
                                    return Ok(comb);
                                } else {
                                    let cmdline = format!("sudo {}", args.join(" "));
                                    let err_text = if comb.trim().is_empty() {
                                        format!("sudo exited with status {}\ncommand: {}", out.status, cmdline)
                                    } else {
                                        format!("{}\ncommand: {}", comb, cmdline)
                                    };
                                    return Err(err_text);
                                }
                            }
                            Err(e) => {
                                let cmdline = format!("sudo {}", args.join(" "));
                                return Err(format!("failed to run sudo -A: {}\ncommand: {}", e, cmdline));
                            }
                        }
                    }
                }

                // No external helper found — fall back to creating a temporary
                // askpass helper script that prints the password.
                let mut args = vec!["-A".to_string(), cmd.program.clone()];
                args.extend(cmd.args.clone());
                let ask_res = Command::new("sudo")
                    .args(&args)
                    .env("SUDO_ASKPASS", &tmp_path)
                    .output();

                // Clean up helper and zeroize local copy
                let _ = std::fs::remove_file(&tmp_path);
                local_pw.zeroize();

                match ask_res {
                    Ok(out) => {
                        let mut comb = String::from_utf8_lossy(&out.stdout).to_string();
                        comb.push_str(&String::from_utf8_lossy(&out.stderr));
                        if out.status.success() {
                            return Ok(comb);
                        } else {
                            let cmdline = format!("sudo {}", args.join(" "));
                            let err_text = if comb.trim().is_empty() {
                                format!("sudo exited with status {}\ncommand: {}", out.status, cmdline)
                            } else {
                                format!("{}\ncommand: {}", comb, cmdline)
                            };
                            return Err(err_text);
                        }
                    }
                    Err(e) => {
                        let cmdline = format!("sudo {}", args.join(" "));
                        return Err(format!("failed to run sudo -A: {}\ncommand: {}", e, cmdline));
                    }
                }
            }
        }
    }

    if output.status.success() {
        Ok(combined)
    } else {
        let cmdline = format!("{} {}", launcher, full_args.join(" "));
        // If the command produced no combined output, include exit status
        // plus the exact command we tried, to help debugging "usage: sudo"
        // cases where sudo printed its usage because of malformed args.
        let err_text = if combined.trim().is_empty() {
            format!("{} exited with status {}\ncommand: {}", launcher, output.status, cmdline)
        } else {
            format!("{}\ncommand: {}", combined, cmdline)
        };
        Err(err_text)
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
