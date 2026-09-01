# Universal Package Manager

A single Rust/[egui](https://github.com/emilk/egui) desktop GUI that works
across distros by detecting your native package manager (APT, DNF, Pacman,
Zypper, or APK) at startup and driving it through one common interface —
plus a built-in Flatpak tab. Comes with a dark/light theme toggle.

## Features

- **Auto-detects your distro's package manager** — Debian/Ubuntu (`apt`),
  Fedora/RHEL (`dnf`), Arch (`pacman`, or `yay` if installed — see caveat
  below), openSUSE (`zypper`), Alpine (`apk`).
- **Search & install / remove packages**, refresh the index, upgrade
  everything.
- **View installed packages.**
- **Manage repositories**: list, add, and remove/disable repos for your
  native package manager.
- **Flatpak support**: search, install/remove flatpak apps, update them,
  and manage Flatpak remotes (Flathub etc.) — all through the same UI.
  If Flatpak itself isn't installed, the Flatpak tab offers a button to
  install it via your native package manager, plus a one-click "Add
  Flathub" once it's in.
- **Dark / light theme toggle** in the top bar.
- Privileged operations (install/remove/repo changes) run through
  `pkexec` (falling back to `sudo`) so you get a normal graphical
  authentication prompt. Flatpak operations default to `--user` scope and
  need no elevation at all.

## Installation

### Quick install (recommended)

This pulls `install.sh` from this repo and runs it — it detects your
distro, installs build dependencies, makes sure you have a recent enough
Rust toolchain (installing one via [rustup](https://rustup.rs) if not),
builds the app, and installs it to `~/.local/bin` plus an app-menu entry:

```bash
curl -sSL https://raw.githubusercontent.com/shv63/universal-pm/main/install.sh | bash
```

For a system-wide install to `/usr/local/bin` instead (uses `sudo`):

```bash
curl -sSL https://raw.githubusercontent.com/shv63/universal-pm/main/install.sh | bash -s -- --system
```

Piping a script straight from the internet into `bash` means you're
trusting this repo completely, with no chance to look at what's about to
run on your machine first. If you'd rather review it before running:

```bash
curl -sSLO https://raw.githubusercontent.com/shv63/universal-pm/main/install.sh
less install.sh        # read it
chmod +x install.sh
./install.sh           # or ./install.sh --system
```

Re-running the script later (either way) pulls the latest `main` and
rebuilds, so it doubles as an updater.

### Updating

To update to the latest `main`, just run the same command again — it
detects the existing checkout, `git pull`s, and rebuilds in place:

```bash
curl -sSL https://raw.githubusercontent.com/shv63/universal-pm/main/install.sh | bash
```

Or, if you cloned manually:

```bash
cd universal-pm
git pull
cargo build --release
```

### Building manually

If you'd rather not run the installer at all:

You'll need a reasonably recent Rust toolchain (edition 2021, roughly
1.75+ is enough for the code itself, but `eframe`'s dependency tree wants a
current `cargo`/`rustc` — if your distro's packaged Rust is old, install a
current one with [rustup](https://rustup.rs) rather than fighting your
package manager's version):

```bash
curl https://sh.rustup.rs -sSf | sh
```

You'll also need the usual native GUI dev libraries `eframe` links
against. On Debian/Ubuntu:

```bash
sudo apt install libx11-dev libxkbcommon-dev libwayland-dev \
    libgl1-mesa-dev libxcb1-dev pkg-config
```

On Fedora: `sudo dnf install libX11-devel libxkbcommon-devel wayland-devel mesa-libGL-devel`
On Arch: `sudo pacman -S libx11 libxkbcommon wayland mesa`

Then build and run:

```bash
git clone https://github.com/shv63/universal-pm.git
cd universal-pm
cargo build --release
./target/release/universal-pm
```

## Project layout

```
src/
  main.rs           - app entry point, window setup
  app.rs            - all UI (tabs, theming, background task plumbing)
  detect.rs          - picks a native backend by probing for known binaries
  backend/
    mod.rs           - the PackageManager trait + shared types + command runner
    apt.rs            - Debian/Ubuntu
    dnf.rs             - Fedora/RHEL
    pacman.rs          - Arch
    zypper.rs          - openSUSE
    apk.rs             - Alpine
    flatpak.rs         - Flatpak (modeled as just another backend)
```

### How the abstraction works

Every distro backend implements one `PackageManager` trait
(`src/backend/mod.rs`): `search`, `list_installed`, `install_cmd`,
`remove_cmd`, `update_index_cmd`, `upgrade_all_cmd`, `list_repos`,
`add_repo_cmd`, `remove_repo_cmd`. The UI (`app.rs`) only ever talks to
`Box<dyn PackageManager>` / `Arc<dyn PackageManager>` — it has no
distro-specific code at all. `detect.rs` just decides *which*
implementation to hand it. Flatpak is implemented against the exact same
trait (its "packages" are app IDs, its "repos" are remotes), which is why
it slots into the same UI tabs with zero special-casing.

All commands (search excepted) run on a background thread and report back
over an `mpsc` channel, so the UI never freezes while `apt-get` or
`flatpak` is doing its thing — check the **Log** tab for full command
output, including errors.

## Notes & things you may want to tweak

- **AUR via `yay`**: if `yay` is installed, it's preferred over plain
  `pacman` automatically (it's a superset — official repos + AUR). Its
  search results include AUR packages. One real limitation: AUR builds
  run through `makepkg`, which refuses to run as root, so unlike every
  other backend here, `yay` commands are **not** wrapped in `pkexec`/
  `sudo` — yay handles elevation internally. That means install/remove/
  upgrade need passwordless `sudo` configured for `pacman` (see the
  comment at the top of `backend/yay.rs` for the one-line `visudo`
  addition), or they'll hang waiting for a password prompt this GUI can't
  show. If you don't want to set that up, everything still works for
  browsing/searching — just do privileged operations from a terminal.
- **Adding a repo** is necessarily distro-specific in format (a
  `ppa:user/name` or full `deb ...` line for APT, a URL for DNF/Zypper, a
  `[name]` / `Server = ...` block for Pacman) — the Repositories tab shows
  a hint for your detected backend.
- Removing a DNF repo *disables* it rather than deleting the `.repo`
  file, so it's reversible. Everything else genuinely removes the entry.
  Adjust `remove_repo_cmd` in `backend/dnf.rs` if you'd rather it deleted
  outright.
- Flatpak defaults to `--user` installs (no root prompt required). Flip
  `SYSTEM_WIDE` to `true` at the top of `backend/flatpak.rs` if you'd
  rather manage system-wide flatpaks (this will then need `pkexec`/root).
- This is a from-scratch tool, not a wrapper around GNOME Software /
  Discover — it shells out to the CLI tools directly, so it works the
  same on any desktop environment (or none).
