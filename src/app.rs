use crate::backend::{self, PackageInfo, PackageManager, RepoInfo};
use eframe::egui;
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

#[derive(PartialEq, Clone, Copy, Debug)]
enum Tab {
    Search,
    Installed,
    Repos,
    Flatpak,
    FlatpakRepos,
    Log,
}

#[derive(Clone, Copy, PartialEq)]
enum Scope {
    Native,
    Flatpak,
}

enum Msg {
    SearchResults(Scope, Vec<PackageInfo>),
    InstalledList(Scope, Vec<PackageInfo>),
    RepoList(Scope, Vec<RepoInfo>),
    Log(String),
    Error(String),
    RequirePassword(Scope, backend::PmCommand, String),
    Done,
}

pub struct App {
    native: Arc<dyn PackageManager>,
    flatpak: Arc<dyn PackageManager>,
    flatpak_available: bool,

    tab: Tab,
    dark_mode: bool,

    search_query: String,
    search_results: Vec<PackageInfo>,
    installed: Vec<PackageInfo>,
    repos: Vec<RepoInfo>,

    fp_query: String,
    fp_results: Vec<PackageInfo>,
    fp_repos: Vec<RepoInfo>,

    new_repo_input: String,
    new_fp_repo_input: String,

    // UI state
    fp_system: bool,
    select_mode: bool,
    selected: HashSet<String>,

    // Password prompt state
    show_pw_prompt: bool,
    pw_input: String,
    pending_cmd: Option<(Scope, backend::PmCommand, String)>,

    log: String,
    busy: bool,

    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let native: Arc<dyn PackageManager> = Arc::from(crate::detect::detect_native_backend());
        let flatpak: Arc<dyn PackageManager> = Arc::new(backend::flatpak::Flatpak);
        Self {
            flatpak_available: crate::detect::flatpak_available(),
            native,
            flatpak,
            tab: Tab::Search,
            dark_mode: true,
            search_query: String::new(),
            search_results: Vec::new(),
            installed: Vec::new(),
            repos: Vec::new(),
            fp_query: String::new(),
            fp_results: Vec::new(),
            fp_repos: Vec::new(),
            new_repo_input: String::new(),
            new_fp_repo_input: String::new(),
            fp_system: false,
            select_mode: false,
            selected: HashSet::new(),
            show_pw_prompt: false,
            pw_input: String::new(),
            pending_cmd: None,
            log: String::new(),
            busy: false,
            tx,
            rx,
        }
    }

    fn append_log(&mut self, s: &str) {
        self.log.push_str(s);
        if !s.ends_with('\n') {
            self.log.push('\n');
        }
    }

    fn backend_for(&self, scope: Scope) -> Arc<dyn PackageManager> {
        match scope {
            Scope::Native => Arc::clone(&self.native),
            Scope::Flatpak => Arc::clone(&self.flatpak),
        }
    }

    fn spawn_search(&mut self, scope: Scope, query: String, system: bool) {
        if query.trim().is_empty() {
            return;
        }
        self.busy = true;
        let pm = self.backend_for(scope);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            match pm.search(&query, system) {
                Ok(results) => {
                    let _ = tx.send(Msg::SearchResults(scope, results));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(e));
                }
            }
            let _ = tx.send(Msg::Done);
        });
    }

    fn spawn_list_installed(&mut self, scope: Scope, system: bool, password: Option<String>) {
        // Special-case: when asking for Flatpak system-installed packages and
        // no password was supplied, don't call into the backend's
        // list_installed (which may call execute and try pkexec/sudo without
        // prompting). Instead present the password modal so the user can
        // provide one and we can run `sudo -S` safely.
        if scope == Scope::Flatpak && system && password.is_none() {
            let label = "list flatpaks (system)".to_string();
            let cmd = backend::PmCommand::new(
                "flatpak",
                &["list", "--system", "--app", "--columns=application,name,version"],
                true,
            );
            self.pending_cmd = Some((scope, cmd, label));
            self.show_pw_prompt = true;
            return;
        }

        self.busy = true;
        let pm = self.backend_for(scope);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // If a password was supplied, run the underlying program via execute
            // (which supports feeding the password to sudo -S). Otherwise call
            // the backend's list_installed method which may call run_capture.
            let res = if password.is_some() {
                // build a generic command for `list` — for Flatpak this is
                // flatpak list --system/--user --app --columns=application,name,version
                // For other backends, fall back to list_installed() since we
                // don't have a generic command.
                match scope {
                    Scope::Flatpak => {
                        let cmd = backend::PmCommand::new(
                            "flatpak",
                            &["list", if system { "--system" } else { "--user" }, "--app", "--columns=application,name,version"],
                            true,
                        ).with_password(password.unwrap());
                        backend::execute(&cmd).and_then(|out| {
                            // parse into Vec<PackageInfo>
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
                        })
                    }
                    _ => pm.list_installed(system),
                }
            } else {
                pm.list_installed(system)
            };

            match res {
                Ok(results) => {
                    let _ = tx.send(Msg::InstalledList(scope, results));
                }
                Err(e) => {
                    // Detect common sudo/prompt errors and ask the UI to request
                    // a password so we can retry with sudo -S instead of
                    // failing silently. Only special-case Flatpak here.
                    let low = e.to_lowercase();
                    if scope == Scope::Flatpak && (low.contains("terminal is required") || low.contains("password is required") || low.contains("a password is required")) {
                        let label = "list flatpaks (system)".to_string();
                        let cmd = backend::PmCommand::new(
                            "flatpak",
                            &["list", if system { "--system" } else { "--user" }, "--app", "--columns=application,name,version"],
                            true,
                        );
                        let _ = tx.send(Msg::RequirePassword(scope, cmd, label));
                    } else {
                        let _ = tx.send(Msg::Error(e));
                    }
                }
            }
            let _ = tx.send(Msg::Done);
        });
    }

    fn spawn_list_repos(&mut self, scope: Scope, system: bool, password: Option<String>) {
        // If asking for system flatpak repos and no password was supplied,
        // present the password modal rather than letting the backend call
        // into execute (which may try pkexec/sudo without prompting).
        if scope == Scope::Flatpak && system && password.is_none() {
            let label = "list flatpak remotes (system)".to_string();
            let cmd = backend::PmCommand::new("flatpak", &["remotes", "--system", "--columns=name,url,title"], true);
            self.pending_cmd = Some((scope, cmd, label));
            self.show_pw_prompt = true;
            return;
        }

        self.busy = true;
        let pm = self.backend_for(scope);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = if password.is_some() {
                match scope {
                    Scope::Flatpak => {
                        let cmd = backend::PmCommand::new(
                            "flatpak",
                            &["remotes", if system { "--system" } else { "--user" }, "--columns=name,url,title"],
                            true,
                        ).with_password(password.unwrap());
                        backend::execute(&cmd).and_then(|out| {
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
                                    enabled: cols.get(3).map(|s| !s.trim().eq_ignore_ascii_case("true")).unwrap_or(true),
                                });
                            }
                            Ok(repos)
                        })
                    }
                    _ => pm.list_repos(system),
                }
            } else {
                pm.list_repos(system)
            };

            match res {
                Ok(results) => {
                    let _ = tx.send(Msg::RepoList(scope, results));
                }
                Err(e) => {
                    let low = e.to_lowercase();
                    if scope == Scope::Flatpak && (low.contains("terminal is required") || low.contains("password is required") || low.contains("a password is required")) {
                        let label = "list flatpak remotes (system)".to_string();
                        let cmd = backend::PmCommand::new(
                            "flatpak",
                            &["remotes", if system { "--system" } else { "--user" }, "--columns=name,url,title"],
                            true,
                        );
                        let _ = tx.send(Msg::RequirePassword(scope, cmd, label));
                    } else {
                        let _ = tx.send(Msg::Error(e));
                    }
                }
            }
            let _ = tx.send(Msg::Done);
        });
    }

    /// Runs any PmCommand-producing closure (install/remove/update/upgrade/
    /// add-repo/remove-repo) in the background and logs the outcome.
    fn spawn_action(&mut self, scope: Scope, label: String, cmd: backend::PmCommand) {
        // If the command requires root but has no password attached, prompt
        // the user for one so it can be fed to sudo -S. Otherwise execute.
        if cmd.needs_root && cmd.password.is_none() {
            self.pending_cmd = Some((scope, cmd, label));
            self.show_pw_prompt = true;
            return;
        }

        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Log(format!("$ {}\n", label)));
            // Execute the command. After running, if the PmCommand carried a
            // password, take it and zeroize it so the plaintext doesn't linger
            // in memory longer than necessary.
            match backend::execute(&cmd) {
                Ok(out) => {
                    let _ = tx.send(Msg::Log(out));
                    let _ = tx.send(Msg::Log(format!("-- {label}: done --\n")));
                }
                Err(e) => {
                    // If sudo complains it needs a TTY or a password, prompt the
                    // user for it instead of just logging an error. This can
                    // happen when sudo is configured to require a tty (requiring
                    // an interactive prompt) or when pkexec isn't available.
                    let low = e.to_lowercase();
                    if low.contains("terminal is required") || low.contains("password is required") || low.contains("a password is required") {
                        // Ask the UI to request a password and retry this command.
                        let _ = tx.send(Msg::RequirePassword(scope, cmd.clone(), label.clone()));
                    } else {
                        let _ = tx.send(Msg::Error(format!("{label} failed: {e}")));
                    }
                }
            }

            // Zeroize any password still stored on the moved-in command (best
            // effort: we take ownership of the Option<String> and zeroize it).
            if let Some(mut pw) = cmd.password {
                use zeroize::Zeroize;
                pw.zeroize();
            }

            let _ = tx.send(Msg::Done);
        });

        // Clear the UI-side password buffer now that the action is launched.
        // Use zeroize to avoid leaving the password in memory.
        if !self.pw_input.is_empty() {
            use zeroize::Zeroize;
            self.pw_input.zeroize();
            self.pw_input.clear();
        }
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::SearchResults(Scope::Native, r) => self.search_results = r,
                Msg::SearchResults(Scope::Flatpak, r) => self.fp_results = r,
                Msg::InstalledList(Scope::Native, r) => self.installed = r,
                Msg::InstalledList(Scope::Flatpak, r) => self.fp_results = r,
                Msg::RepoList(Scope::Native, r) => self.repos = r,
                Msg::RepoList(Scope::Flatpak, r) => self.fp_repos = r,
                Msg::Log(s) => self.append_log(&s),
                Msg::Error(e) => self.append_log(&format!("ERROR: {e}")),
                Msg::RequirePassword(scope, cmd, label) => {
                    // Show the password modal and hold the pending command so the
                    // user can enter a password to retry the action.
                    self.pending_cmd = Some((scope, cmd, label));
                    self.show_pw_prompt = true;
                    self.append_log("Authentication required: prompting for password...");
                }
                Msg::Done => {
                    self.busy = false;
                    self.flatpak_available = crate::detect::flatpak_available();
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_messages();
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }

        ctx.set_visuals(if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Universal Package Manager");
                ui.separator();
                ui.label(format!("Backend: {}", self.native.display_name()));
                if self.flatpak_available {
                    ui.colored_label(egui::Color32::from_rgb(120, 190, 120), "Flatpak ✓");
                } else {
                    ui.colored_label(egui::Color32::from_rgb(190, 120, 120), "Flatpak not found");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let icon = if self.dark_mode { "🌙" } else { "☀" };
                    if ui.button(icon).on_hover_text("Toggle dark/light theme").clicked() {
                        self.dark_mode = !self.dark_mode;
                    }
                    if self.busy {
                        ui.spinner();
                        ui.label("working…");
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Search, "🔍 Search / Install");
                ui.selectable_value(&mut self.tab, Tab::Installed, "📦 Installed");
                ui.selectable_value(&mut self.tab, Tab::Repos, "🗂 Repositories");
                ui.selectable_value(&mut self.tab, Tab::Flatpak, "▶ Flatpak");
                ui.selectable_value(&mut self.tab, Tab::FlatpakRepos, "▶ Flatpak Remotes");
                ui.selectable_value(&mut self.tab, Tab::Log, "🖥 Log");
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Search => self.ui_search(ui),
            Tab::Installed => self.ui_installed(ui),
            Tab::Repos => self.ui_repos(ui),
            Tab::Flatpak => self.ui_flatpak(ui),
            Tab::FlatpakRepos => self.ui_flatpak_repos(ui),
            Tab::Log => self.ui_log(ui),
        });

        // Password prompt modal: when a privileged command is about to run
        // and no password was supplied, show a prompt to enter one so it can
        // be fed to sudo -S. The user can cancel as well.
        if self.show_pw_prompt {
            let mut open = self.show_pw_prompt;
            egui::Window::new("Enter root password (or leave blank to use pkexec)")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Some operations require root privileges.");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Password:");
                        let resp = ui.add(egui::TextEdit::singleline(&mut self.pw_input).password(true));
                        // Request keyboard focus for a smoother UX when the modal opens.
                        resp.request_focus();
                        // If the user presses Enter in the field, submit the form.
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            // emulate pressing Run
                            if let Some((scope, mut cmd, label)) = self.pending_cmd.take() {
                                if !self.pw_input.is_empty() {
                                    cmd = cmd.with_password(self.pw_input.clone());
                                }
                                self.show_pw_prompt = false;
                                self.pending_cmd = None;
                                self.spawn_action(scope, label, cmd);
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Run").clicked() {
                            if let Some((scope, mut cmd, label)) = self.pending_cmd.take() {
                                if !self.pw_input.is_empty() {
                                    cmd = cmd.with_password(self.pw_input.clone());
                                }
                                self.show_pw_prompt = false;
                                self.pending_cmd = None;
                                self.spawn_action(scope, label, cmd);
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_pw_prompt = false;
                            self.pending_cmd = None;
                        }
                    });
                });
            self.show_pw_prompt = open;
        }
    }
}

impl App {
    fn ui_search(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search:");
            let resp = ui.text_edit_singleline(&mut self.search_query);
            let go = ui.button("Search").clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go {
                self.spawn_search(Scope::Native, self.search_query.clone(), false);
            }
            ui.separator();
            if ui.button("⟲ Refresh package index").clicked() {
                let cmd_label = "update package index".to_string();
                let cmd = self.backend_for(Scope::Native).update_index_cmd(false);
                self.spawn_action(Scope::Native, cmd_label, cmd);
            }
            if ui.button("⬆ Upgrade all").clicked() {
                let cmd_label = "upgrade all packages".to_string();
                let cmd = self.backend_for(Scope::Native).upgrade_all_cmd(false);
                self.spawn_action(Scope::Native, cmd_label, cmd);
            }
            ui.separator();
            ui.checkbox(&mut self.select_mode, "Select packages");
            if self.select_mode {
                if ui.button("Install Selected").clicked() {
                    let names: Vec<String> = self.selected.iter().cloned().collect();
                    self.selected.clear();
                    for name in names {
                        let label = format!("install {name}");
                        let cmd = self.backend_for(Scope::Native).install_cmd(&name, false);
                        self.spawn_action(Scope::Native, label, cmd);
                    }
                }
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            let cols = if self.select_mode { 5 } else { 4 };
            egui::Grid::new("search_results_grid")
                .num_columns(cols)
                .striped(true)
                .show(ui, |ui| {
                    if self.select_mode { ui.strong(""); }
                    ui.strong("Name");
                    ui.strong("Description");
                    ui.strong("Status");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_install: Option<String> = None;
                    let mut to_remove: Option<String> = None;
                    for pkg in &self.search_results {
                        if self.select_mode {
                            let mut checked = self.selected.contains(&pkg.name);
                            if ui.checkbox(&mut checked, "").clicked() {
                                if checked {
                                    self.selected.insert(pkg.name.clone());
                                } else {
                                    self.selected.remove(&pkg.name);
                                }
                            }
                        }
                        ui.label(&pkg.name);
                        ui.label(&pkg.description);
                        ui.label(if pkg.installed { "installed" } else { "" });
                        if pkg.installed {
                            if ui.button("Remove").clicked() {
                                to_remove = Some(pkg.name.clone());
                            }
                        } else if ui.button("Install").clicked() {
                            to_install = Some(pkg.name.clone());
                        }
                        ui.end_row();
                    }
                    if let Some(name) = to_install {
                        let label = format!("install {name}");
                        let cmd = self.backend_for(Scope::Native).install_cmd(&name, false);
                        self.spawn_action(Scope::Native, label, cmd);
                    }
                    if let Some(name) = to_remove {
                        let label = format!("remove {name}");
                        let cmd = self.backend_for(Scope::Native).remove_cmd(&name, false);
                        self.spawn_action(Scope::Native, label, cmd);
                    }
                });
        });
    }

    fn ui_installed(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⟲ Refresh installed list").clicked() {
                self.spawn_list_installed(Scope::Native, false, None);
            }
            ui.label(format!("{} packages installed", self.installed.len()));
            ui.separator();
            ui.checkbox(&mut self.select_mode, "Select packages");
            if self.select_mode {
                if ui.button("Remove Selected").clicked() {
                    let names: Vec<String> = self.selected.iter().cloned().collect();
                    self.selected.clear();
                    for name in names {
                        let label = format!("remove {name}");
                        let cmd = self.backend_for(Scope::Native).remove_cmd(&name, false);
                        self.spawn_action(Scope::Native, label, cmd);
                    }
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let cols = if self.select_mode { 5 } else { 4 };
            egui::Grid::new("installed_grid")
                .num_columns(cols)
                .striped(true)
                .show(ui, |ui| {
                    if self.select_mode { ui.strong(""); }
                    ui.strong("Name");
                    ui.strong("Version");
                    ui.strong("Description");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_remove: Option<String> = None;
                    for pkg in &self.installed {
                        if self.select_mode {
                            let mut checked = self.selected.contains(&pkg.name);
                            if ui.checkbox(&mut checked, "").clicked() {
                                if checked {
                                    self.selected.insert(pkg.name.clone());
                                } else {
                                    self.selected.remove(&pkg.name);
                                }
                            }
                        }
                        ui.label(&pkg.name);
                        ui.label(&pkg.version);
                        ui.label(&pkg.description);
                        if ui.button("Remove").clicked() {
                            to_remove = Some(pkg.name.clone());
                        }
                        ui.end_row();
                    }
                    if let Some(name) = to_remove {
                        let label = format!("remove {name}");
                        let cmd = self.backend_for(Scope::Native).remove_cmd(&name, false);
                        self.spawn_action(Scope::Native, label, cmd);
                    }
                });
        });
    }

    fn ui_repos(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⟲ Refresh repo list").clicked() {
                self.spawn_list_repos(Scope::Native, false, None);
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Add repo:");
            ui.text_edit_singleline(&mut self.new_repo_input);
            if ui.button("Add").clicked() && !self.new_repo_input.trim().is_empty() {
                let repo = self.new_repo_input.clone();
                let label = format!("add repo {repo}");
                let cmd = self.backend_for(Scope::Native).add_repo_cmd(&repo, false);
                self.spawn_action(Scope::Native, label, cmd);
                self.new_repo_input.clear();
            }
        });
        ui.small(
            "Format depends on your distro: a PPA (\"ppa:user/name\") or `deb ...` line on \
             Debian/Ubuntu, a URL on Fedora/openSUSE, or a `[name]`/`Server = ...` block on Arch.",
        );
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("repos_grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("URL / Info");
                    ui.strong("Enabled");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_remove: Option<String> = None;
                    for repo in &self.repos {
                        ui.label(&repo.name);
                        ui.label(&repo.url);
                        ui.label(if repo.enabled { "yes" } else { "no" });
                        if ui.button("Remove").clicked() {
                            to_remove = Some(repo.id.clone());
                        }
                        ui.end_row();
                    }
                    if let Some(id) = to_remove {
                        let label = format!("remove repo {id}");
                        let cmd = self.backend_for(Scope::Native).remove_repo_cmd(&id, true);
                        self.spawn_action(Scope::Native, label, cmd);
                    }
                });
        });
    }

    /// Shown on both Flatpak tabs when the `flatpak` binary isn't found —
    /// offers to install it through the detected native package manager
    /// instead of just being a dead end.
    fn ui_flatpak_missing(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        ui.label("Flatpak isn't installed on this system.");
        ui.add_space(6.0);
        if ui
            .button(format!(
                "Install Flatpak via {}",
                self.native.display_name()
            ))
            .clicked()
        {
            let label = "install flatpak".to_string();
        let cmd = self.backend_for(Scope::Native).install_cmd("flatpak", true);
        self.spawn_action(Scope::Native, label, cmd);
        }
        ui.add_space(4.0);
        ui.small(
            "This runs your native package manager's install command for the \
             \"flatpak\" package (elevated via pkexec/sudo). Once it finishes, \
             this tab will switch over automatically — you'll likely still want \
             to add the Flathub remote afterward from the Flatpak Remotes tab.",
        );
    }

    fn ui_flatpak(&mut self, ui: &mut egui::Ui) {
        if !self.flatpak_available {
            self.ui_flatpak_missing(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Search:");
            let resp = ui.text_edit_singleline(&mut self.fp_query);
            let go = ui.button("Search").clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go {
                self.spawn_search(Scope::Flatpak, self.fp_query.clone(), self.fp_system);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.fp_system, false, "User");
                ui.radio_value(&mut self.fp_system, true, "System");
            });
            if self.fp_system {
                ui.add(egui::widgets::Label::new("System actions require root: provide password below (optional: use pkexec)").wrap());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.pw_input).password(true));
                    if ui.button("List installed (system)").clicked() {
                        let pw = if self.pw_input.is_empty() { None } else { Some(self.pw_input.clone()) };
                        self.spawn_list_installed(Scope::Flatpak, true, pw);
                    }
                    if ui.button("Refresh remotes (system)").clicked() {
                        let pw = if self.pw_input.is_empty() { None } else { Some(self.pw_input.clone()) };
                        self.spawn_list_repos(Scope::Flatpak, true, pw);
                    }
                });
            } else {
                if ui.button("📋 List installed").clicked() {
                    self.spawn_list_installed(Scope::Flatpak, false, None);
                }
                if ui.button("⟲ Refresh remotes").clicked() {
                    self.spawn_list_repos(Scope::Flatpak, false, None);
                }
            }
            if ui.button("⬆ Update all flatpaks").clicked() {
                let label = "update all flatpaks".to_string();
                let cmd = self.backend_for(Scope::Flatpak).upgrade_all_cmd(self.fp_system);
                self.spawn_action(Scope::Flatpak, label, cmd);
            }
            ui.separator();
            ui.checkbox(&mut self.select_mode, "Select packages");
            if self.select_mode {
                if ui.button("Install Selected").clicked() {
                    let names: Vec<String> = self.selected.iter().cloned().collect();
                    self.selected.clear();
                    for name in names {
                        let label = format!("install flatpak {name}");
                        let cmd = self.backend_for(Scope::Flatpak).install_cmd(&name, self.fp_system);
                        if self.fp_system && !self.pw_input.is_empty() {
                            let cmd = cmd.with_password(self.pw_input.clone());
                            self.spawn_action(Scope::Flatpak, label, cmd);
                        } else {
                            self.spawn_action(Scope::Flatpak, label, cmd);
                        }
                    }
                }
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            let cols = if self.select_mode { 5 } else { 4 };
            egui::Grid::new("flatpak_grid")
                .num_columns(cols)
                .striped(true)
                .show(ui, |ui| {
                    if self.select_mode { ui.strong(""); }
                    ui.strong("App ID");
                    ui.strong("Name / Version");
                    ui.strong("Description");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_install: Option<String> = None;
                    let mut to_remove: Option<String> = None;
                    for pkg in &self.fp_results {
                        if self.select_mode {
                            let mut checked = self.selected.contains(&pkg.name);
                            if ui.checkbox(&mut checked, "").clicked() {
                                if checked {
                                    self.selected.insert(pkg.name.clone());
                                } else {
                                    self.selected.remove(&pkg.name);
                                }
                            }
                        }
                        ui.label(&pkg.name);
                        ui.label(&pkg.version);
                        ui.label(&pkg.description);
                        if pkg.installed {
                            if ui.button("Remove").clicked() {
                                to_remove = Some(pkg.name.clone());
                            }
                        } else if ui.button("Install").clicked() {
                            to_install = Some(pkg.name.clone());
                        }
                        ui.end_row();
                    }
                    if let Some(id) = to_install {
                        let label = format!("install flatpak {id}");
                        let cmd = self.backend_for(Scope::Flatpak).install_cmd(&id, self.fp_system);
                        let cmd = if self.fp_system && !self.pw_input.is_empty() { cmd.with_password(self.pw_input.clone()) } else { cmd };
                        self.spawn_action(Scope::Flatpak, label, cmd);
                    }
                    if let Some(id) = to_remove {
                        let label = format!("remove flatpak {id}");
                        let cmd = self.backend_for(Scope::Flatpak).remove_cmd(&id, self.fp_system);
                        let cmd = if self.fp_system && !self.pw_input.is_empty() { cmd.with_password(self.pw_input.clone()) } else { cmd };
                        self.spawn_action(Scope::Flatpak, label, cmd);
                    }
                });
        });
    }

    fn ui_flatpak_repos(&mut self, ui: &mut egui::Ui) {
        if !self.flatpak_available {
            self.ui_flatpak_missing(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.fp_system, false, "User");
                ui.radio_value(&mut self.fp_system, true, "System");
            });
            if ui.button("⟲ Refresh remotes").clicked() {
                let pw = if self.fp_system && !self.pw_input.is_empty() { Some(self.pw_input.clone()) } else { None };
                self.spawn_list_repos(Scope::Flatpak, self.fp_system, pw);
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Add remote (\"name url\"):");
            ui.text_edit_singleline(&mut self.new_fp_repo_input);
            if ui.button("Add").clicked() && !self.new_fp_repo_input.trim().is_empty() {
                let repo = self.new_fp_repo_input.clone();
                let label = format!("add flatpak remote {repo}");
                let mut cmd = self.backend_for(Scope::Flatpak).add_repo_cmd(&repo, self.fp_system);
                if self.fp_system && !self.pw_input.is_empty() {
                    cmd = cmd.with_password(self.pw_input.clone());
                }
                self.spawn_action(Scope::Flatpak, label, cmd);
                self.new_fp_repo_input.clear();
            }
            ui.separator();
            if ui.button("+ Add Flathub").clicked() {
                let repo = "flathub https://flathub.org/repo/flathub.flatpakrepo".to_string();
                let label = "add flatpak remote flathub".to_string();
                let mut cmd = self.backend_for(Scope::Flatpak).add_repo_cmd(&repo, self.fp_system);
                if self.fp_system && !self.pw_input.is_empty() {
                    cmd = cmd.with_password(self.pw_input.clone());
                }
                self.spawn_action(Scope::Flatpak, label, cmd);
            }
        });
        ui.small(
            "e.g. flathub https://flathub.org/repo/flathub.flatpakrepo",
        );
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("fp_repos_grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("URL");
                    ui.strong("Enabled");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_remove: Option<String> = None;
                    for repo in &self.fp_repos {
                        ui.label(&repo.name);
                        ui.label(&repo.url);
                        ui.label(if repo.enabled { "yes" } else { "no" });
                        if ui.button("Remove").clicked() {
                            to_remove = Some(repo.id.clone());
                        }
                        ui.end_row();
                    }
                    if let Some(id) = to_remove {
                        let label = format!("remove flatpak remote {id}");
                        let mut cmd = self.backend_for(Scope::Flatpak).remove_repo_cmd(&id, self.fp_system);
                        if self.fp_system && !self.pw_input.is_empty() {
                            cmd = cmd.with_password(self.pw_input.clone());
                        }
                        self.spawn_action(Scope::Flatpak, label, cmd);
                    }
                });
        });
    }

    fn ui_log(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Command Log");
            if ui.button("Clear").clicked() {
                self.log.clear();
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.log)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
        });
    }
}
