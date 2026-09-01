use crate::backend::{
    apk::Apk, apt::Apt, dnf::Dnf, pacman::Pacman, which, yay::Yay, zypper::Zypper, PackageManager,
};

/// Picks the native package manager for whatever distro we're running on
/// by probing for known binaries, in a sensible priority order for distros
/// that ship more than one (e.g. Fedora historically shipped both `yum`
/// and `dnf`; we always prefer the modern one). On Arch, `yay` is preferred
/// over plain `pacman` when installed, since it's a strict superset
/// (AUR + official repos) — see backend/yay.rs for the root-privilege
/// caveat that comes with that choice.
pub fn detect_native_backend() -> Box<dyn PackageManager> {
    let candidates: &[(&str, fn() -> Box<dyn PackageManager>)] = &[
        ("apt-get", || Box::new(Apt)),
        ("apt", || Box::new(Apt)),
        ("dnf", || Box::new(Dnf)),
        ("yay", || Box::new(Yay)),
        ("pacman", || Box::new(Pacman)),
        ("zypper", || Box::new(Zypper)),
        ("apk", || Box::new(Apk)),
    ];
    for (bin, ctor) in candidates {
        if which(bin) {
            return ctor();
        }
    }
    // Fall back to Apt's non-fatal error paths if nothing was detected;
    // every call will simply report "command not found".
    Box::new(Apt)
}

pub fn flatpak_available() -> bool {
    which("flatpak")
}
