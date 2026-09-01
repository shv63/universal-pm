use crate::backend::{apk::Apk, apt::Apt, dnf::Dnf, pacman::Pacman, which, zypper::Zypper, PackageManager};

/// Picks the native package manager for whatever distro we're running on
/// by probing for known binaries, in a sensible priority order for distros
/// that ship more than one (e.g. Fedora historically shipped both `yum`
/// and `dnf`; we always prefer the modern one).
pub fn detect_native_backend() -> Box<dyn PackageManager> {
    let candidates: &[(&str, fn() -> Box<dyn PackageManager>)] = &[
        ("apt-get", || Box::new(Apt)),
        ("apt", || Box::new(Apt)),
        ("dnf", || Box::new(Dnf)),
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
