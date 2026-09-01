use super::{run_capture, PackageInfo, PackageManager, PmCommand, RepoInfo};
use std::fs;

pub struct Apk;

fn parse_verbose_listing(out: &str, installed: bool) -> Vec<PackageInfo> {
    // Lines look like: "pkgname-1.2.3-r0 - description text"
    let mut results = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (name_ver, desc) = line.split_once(" - ").unwrap_or((line, ""));
        // Strip the trailing "-<version>-r<rev>" to get a bare name.
        let name = match name_ver.rfind('-').and_then(|i| {
            name_ver[..i].rfind('-').map(|j| (j, i))
        }) {
            Some((j, _)) => name_ver[..j].to_string(),
            None => name_ver.to_string(),
        };
        results.push(PackageInfo {
            name,
            version: String::new(),
            description: desc.to_string(),
            installed,
        });
    }
    results
}

impl PackageManager for Apk {
    fn display_name(&self) -> &'static str {
        "APK (Alpine)"
    }

    fn binary(&self) -> &'static str {
        "apk"
    }

    fn search(&self, query: &str) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("apk", &["search", "-v", query])?;
        let installed_out = run_capture("apk", &["info", "-v"]).unwrap_or_default();
        let installed_names: Vec<String> = parse_verbose_listing(&installed_out, true)
            .into_iter()
            .map(|p| p.name)
            .collect();
        let mut results = parse_verbose_listing(&out, false);
        for r in &mut results {
            r.installed = installed_names.contains(&r.name);
        }
        Ok(results)
    }

    fn list_installed(&self) -> Result<Vec<PackageInfo>, String> {
        let out = run_capture("apk", &["info", "-v"])?;
        Ok(parse_verbose_listing(&out, true))
    }

    fn install_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new("apk", &["add", pkg], true)
    }

    fn remove_cmd(&self, pkg: &str) -> PmCommand {
        PmCommand::new("apk", &["del", pkg], true)
    }

    fn update_index_cmd(&self) -> PmCommand {
        PmCommand::new("apk", &["update"], true)
    }

    fn upgrade_all_cmd(&self) -> PmCommand {
        PmCommand::new("apk", &["upgrade"], true)
    }

    fn list_repos(&self) -> Result<Vec<RepoInfo>, String> {
        let content = fs::read_to_string("/etc/apk/repositories").unwrap_or_default();
        let mut repos = Vec::new();
        for (i, raw) in content.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let enabled = !line.starts_with('#');
            let url = line.trim_start_matches('#').trim();
            repos.push(RepoInfo {
                id: format!("{i}"),
                name: url.to_string(),
                url: url.to_string(),
                enabled,
            });
        }
        Ok(repos)
    }

    fn add_repo_cmd(&self, repo: &str) -> PmCommand {
        PmCommand::new(
            "sh",
            &[
                "-c",
                &format!("echo {} >> /etc/apk/repositories", shell_quote(repo)),
            ],
            true,
        )
    }

    fn remove_repo_cmd(&self, repo_id: &str) -> PmCommand {
        // repo_id is the line's URL (list_repos uses the URL as name/id
        // for apk since the file has no separate alias).
        let script = format!(
            "grep -vF {} /etc/apk/repositories > /tmp/apk.repos.new && mv /tmp/apk.repos.new /etc/apk/repositories",
            shell_quote(repo_id)
        );
        PmCommand::new("sh", &["-c", &script], true)
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
