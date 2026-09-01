use crate::backend::{self, PackageInfo, PackageManager, RepoInfo};
use eframe::egui;
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

    fn spawn_search(&mut self, scope: Scope, query: String) {
        if query.trim().is_empty() {
            return;
        }
        self.busy = true;
        let pm = self.backend_for(scope);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            match pm.search(&query) {
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

    fn spawn_list_installed(&mut self, scope: Scope) {
        self.busy = true;
        let pm = self.backend_for(scope);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            match pm.list_installed() {
                Ok(results) => {
                    let _ = tx.send(Msg::InstalledList(scope, results));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(e));
                }
            }
            let _ = tx.send(Msg::Done);
        });
    }

    fn spawn_list_repos(&mut self, scope: Scope) {
        self.busy = true;
        let pm = self.backend_for(scope);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            match pm.list_repos() {
                Ok(results) => {
                    let _ = tx.send(Msg::RepoList(scope, results));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(e));
                }
            }
            let _ = tx.send(Msg::Done);
        });
    }

    /// Runs any PmCommand-producing closure (install/remove/update/upgrade/
    /// add-repo/remove-repo) in the background and logs the outcome.
    fn spawn_action<F>(&mut self, scope: Scope, label: String, make_cmd: F)
    where
        F: FnOnce(&dyn PackageManager) -> backend::PmCommand + Send + 'static,
    {
        self.busy = true;
        let pm = self.backend_for(scope);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let cmd = make_cmd(pm.as_ref());
            let _ = tx.send(Msg::Log(format!("$ {}\n", label)));
            match backend::execute(&cmd) {
                Ok(out) => {
                    let _ = tx.send(Msg::Log(out));
                    let _ = tx.send(Msg::Log(format!("-- {label}: done --\n")));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(format!("{label} failed: {e}")));
                }
            }
            let _ = tx.send(Msg::Done);
        });
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
                Msg::Done => self.busy = false,
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
                if self.flatpak_available {
                    ui.selectable_value(&mut self.tab, Tab::Flatpak, "▶ Flatpak");
                    ui.selectable_value(&mut self.tab, Tab::FlatpakRepos, "▶ Flatpak Remotes");
                }
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
                self.spawn_search(Scope::Native, self.search_query.clone());
            }
            ui.separator();
            if ui.button("⟲ Refresh package index").clicked() {
                let cmd_label = "update package index".to_string();
                self.spawn_action(Scope::Native, cmd_label, |pm| pm.update_index_cmd());
            }
            if ui.button("⬆ Upgrade all").clicked() {
                let cmd_label = "upgrade all packages".to_string();
                self.spawn_action(Scope::Native, cmd_label, |pm| pm.upgrade_all_cmd());
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("search_results_grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Description");
                    ui.strong("Status");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_install: Option<String> = None;
                    let mut to_remove: Option<String> = None;
                    for pkg in &self.search_results {
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
                        self.spawn_action(Scope::Native, label, move |pm| pm.install_cmd(&name));
                    }
                    if let Some(name) = to_remove {
                        let label = format!("remove {name}");
                        self.spawn_action(Scope::Native, label, move |pm| pm.remove_cmd(&name));
                    }
                });
        });
    }

    fn ui_installed(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⟲ Refresh installed list").clicked() {
                self.spawn_list_installed(Scope::Native);
            }
            ui.label(format!("{} packages installed", self.installed.len()));
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("installed_grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Version");
                    ui.strong("Description");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_remove: Option<String> = None;
                    for pkg in &self.installed {
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
                        self.spawn_action(Scope::Native, label, move |pm| pm.remove_cmd(&name));
                    }
                });
        });
    }

    fn ui_repos(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⟲ Refresh repo list").clicked() {
                self.spawn_list_repos(Scope::Native);
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Add repo:");
            ui.text_edit_singleline(&mut self.new_repo_input);
            if ui.button("Add").clicked() && !self.new_repo_input.trim().is_empty() {
                let repo = self.new_repo_input.clone();
                let label = format!("add repo {repo}");
                self.spawn_action(Scope::Native, label, move |pm| pm.add_repo_cmd(&repo));
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
                        self.spawn_action(Scope::Native, label, move |pm| pm.remove_repo_cmd(&id));
                    }
                });
        });
    }

    fn ui_flatpak(&mut self, ui: &mut egui::Ui) {
        if !self.flatpak_available {
            ui.label("Flatpak is not installed on this system.");
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Search:");
            let resp = ui.text_edit_singleline(&mut self.fp_query);
            let go = ui.button("Search").clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go {
                self.spawn_search(Scope::Flatpak, self.fp_query.clone());
            }
            ui.separator();
            if ui.button("📋 List installed").clicked() {
                self.spawn_list_installed(Scope::Flatpak);
            }
            if ui.button("⬆ Update all flatpaks").clicked() {
                let label = "update all flatpaks".to_string();
                self.spawn_action(Scope::Flatpak, label, |pm| pm.upgrade_all_cmd());
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("flatpak_grid")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("App ID");
                    ui.strong("Name / Version");
                    ui.strong("Description");
                    ui.strong("Action");
                    ui.end_row();

                    let mut to_install: Option<String> = None;
                    let mut to_remove: Option<String> = None;
                    for pkg in &self.fp_results {
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
                        self.spawn_action(Scope::Flatpak, label, move |pm| pm.install_cmd(&id));
                    }
                    if let Some(id) = to_remove {
                        let label = format!("remove flatpak {id}");
                        self.spawn_action(Scope::Flatpak, label, move |pm| pm.remove_cmd(&id));
                    }
                });
        });
    }

    fn ui_flatpak_repos(&mut self, ui: &mut egui::Ui) {
        if !self.flatpak_available {
            ui.label("Flatpak is not installed on this system.");
            return;
        }
        ui.horizontal(|ui| {
            if ui.button("⟲ Refresh remotes").clicked() {
                self.spawn_list_repos(Scope::Flatpak);
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Add remote (\"name url\"):");
            ui.text_edit_singleline(&mut self.new_fp_repo_input);
            if ui.button("Add").clicked() && !self.new_fp_repo_input.trim().is_empty() {
                let repo = self.new_fp_repo_input.clone();
                let label = format!("add flatpak remote {repo}");
                self.spawn_action(Scope::Flatpak, label, move |pm| pm.add_repo_cmd(&repo));
                self.new_fp_repo_input.clear();
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
                        self.spawn_action(Scope::Flatpak, label, move |pm| pm.remove_repo_cmd(&id));
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
