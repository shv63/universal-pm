#!/usr/bin/env bash
#
# install.sh — builds and installs Universal Package Manager
# (https://github.com/shv63/universal-pm) from source.
#
# Usage:
#   ./install.sh            # install to ~/.local/bin (no root needed)
#   ./install.sh --system   # install to /usr/local/bin (uses sudo)
#
set -euo pipefail

REPO_URL="https://github.com/shv63/universal-pm.git"
BRANCH="main"
BUILD_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/universal-pm-build"
MIN_CARGO_VERSION="1.78.0"

SYSTEM_INSTALL=false
for arg in "$@"; do
    case "$arg" in
        --system) SYSTEM_INSTALL=true ;;
        -h|--help)
            echo "Usage: $0 [--system]"
            echo "  --system   install to /usr/local/bin instead of ~/.local/bin (uses sudo)"
            exit 0
            ;;
    esac
done

log()  { printf '\033[1;34m==>\033[0m %s\n' "$1"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$1"; }
die()  { printf '\033[1;31mERROR:\033[0m %s\n' "$1" >&2; exit 1; }

# ---------------------------------------------------------------------------
# 1. Detect distro package manager (for installing build deps + git/curl)
# ---------------------------------------------------------------------------
PKG_MGR=""
for candidate in apt-get dnf pacman zypper apk; do
    if command -v "$candidate" >/dev/null 2>&1; then
        PKG_MGR="$candidate"
        break
    fi
done
[ -n "$PKG_MGR" ] || die "Couldn't detect a supported package manager (apt/dnf/pacman/zypper/apk)."
log "Detected package manager: $PKG_MGR"

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    elif command -v pkexec >/dev/null 2>&1; then
        pkexec "$@"
    else
        die "Need root to run: $* (no sudo/pkexec found)"
    fi
}

# ---------------------------------------------------------------------------
# 2. Install build dependencies: compiler toolchain bits, git, curl, and the
#    X11/Wayland/GL dev headers eframe links against.
# ---------------------------------------------------------------------------
log "Installing build dependencies (you may be prompted for your password)..."
case "$PKG_MGR" in
    apt-get)
        as_root apt-get update -qq
        as_root apt-get install -y git curl pkg-config build-essential \
            libx11-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev libxcb1-dev
        ;;
    dnf)
        as_root dnf install -y git curl pkgconf-devel gcc gcc-c++ \
            libX11-devel libxkbcommon-devel wayland-devel mesa-libGL-devel
        ;;
    pacman)
        as_root pacman -Sy --noconfirm --needed git curl pkgconf base-devel \
            libx11 libxkbcommon wayland mesa
        ;;
    zypper)
        as_root zypper --non-interactive install git curl pkg-config gcc gcc-c++ \
            libX11-devel libxkbcommon-devel wayland-devel Mesa-libGL-devel
        ;;
    apk)
        as_root apk add git curl pkgconfig build-base \
            libx11-dev libxkbcommon-dev wayland-dev mesa-dev
        ;;
esac

# ---------------------------------------------------------------------------
# 2.5 Ensure an askpass helper exists (so sudo -A can use it). If one of the
# common helpers is available we don't need to install anything; otherwise
# attempt to install a suitable package for the detected package manager.
# ---------------------------------------------------------------------------
if command -v ssh-askpass >/dev/null 2>&1 || command -v x11-ssh-askpass >/dev/null 2>&1 || command -v gnome-ssh-askpass >/dev/null 2>&1 || command -v ksshaskpass >/dev/null 2>&1 || command -v qt5-askpass >/dev/null 2>&1 || command -v openssh-askpass >/dev/null 2>&1; then
    log "Askpass helper already present"
else
    log "No askpass helper found — attempting to install one via $PKG_MGR"
    case "$PKG_MGR" in
        apt-get)
            for pkg in ssh-askpass x11-ssh-askpass ssh-askpass-gnome; do
                if as_root apt-get install -y "$pkg" >/dev/null 2>&1; then
                    log "Installed $pkg"
                    break
                fi
            done
            ;;
        dnf)
            for pkg in openssh-askpass x11-ssh-askpass ssh-askpass; do
                if as_root dnf install -y "$pkg" >/dev/null 2>&1; then
                    log "Installed $pkg"
                    break
                fi
            done
            ;;
        pacman)
            for pkg in ssh-askpass openssh-askpass; do
                if as_root pacman -Sy --noconfirm --needed "$pkg" >/dev/null 2>&1; then
                    log "Installed $pkg"
                    break
                fi
            done
            # If not installed via official repos, try AUR helpers (yay/paru)
            if ! (command -v ssh-askpass >/dev/null 2>&1 || command -v x11-ssh-askpass >/dev/null 2>&1 || command -v gnome-ssh-askpass >/dev/null 2>&1 || command -v ksshaskpass >/dev/null 2>&1 || command -v qt5-askpass >/dev/null 2>&1 || command -v openssh-askpass >/dev/null 2>&1); then
                if command -v yay >/dev/null 2>&1 || command -v paru >/dev/null 2>&1; then
                    AUR_HELPER=$(command -v yay >/dev/null 2>&1 && echo yay || echo paru)
                    log "Attempting to install askpass via AUR helper: $AUR_HELPER"
                    for pkg in openssh-askpass ssh-askpass; do
                        if as_root $AUR_HELPER -S --noconfirm --needed "$pkg" >/dev/null 2>&1; then
                            log "Installed $pkg via $AUR_HELPER"
                            break
                        fi
                    done
                else
                    # Fall back to building from AUR via makepkg if possible
                    if command -v makepkg >/dev/null 2>&1 && command -v git >/dev/null 2>&1; then
                        log "No AUR helper found; attempting to build askpass from AUR (openssh-askpass or ssh-askpass)"
                        for pkg in openssh-askpass ssh-askpass; do
                            tmpd=$(mktemp -d)
                            if git clone "https://aur.archlinux.org/${pkg}.git" "$tmpd" >/dev/null 2>&1; then
                                (cd "$tmpd" && makepkg -si --noconfirm) >/dev/null 2>&1 && { log "Built and installed $pkg from AUR"; rm -rf "$tmpd"; break; } || rm -rf "$tmpd"
                            else
                                rm -rf "$tmpd"
                            fi
                        done
                    else
                        warn "No AUR helper (yay/paru) or makepkg available; could not install an askpass helper on pacman-based system."
                    fi
                fi
            fi
            ;;
        zypper)
            for pkg in openssh-askpass ssh-askpass; do
                if as_root zypper --non-interactive install "$pkg" >/dev/null 2>&1; then
                    log "Installed $pkg"
                    break
                fi
            done
            ;;
        apk)
            for pkg in ssh-askpass; do
                if as_root apk add "$pkg" >/dev/null 2>&1; then
                    log "Installed $pkg"
                    break
                fi
            done
            ;;
    esac
fi

# ---------------------------------------------------------------------------
# 3. Ensure a recent-enough Rust toolchain. Distro-packaged rustc is often
#    too old for eframe's dependency tree, so we prefer rustup.
# ---------------------------------------------------------------------------
version_ge() {
    # returns success if $1 >= $2
    [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n1)" = "$2" ]
}

need_rustup=false
if ! command -v cargo >/dev/null 2>&1; then
    need_rustup=true
else
    cargo_ver="$(cargo --version | awk '{print $2}')"
    if ! version_ge "$cargo_ver" "$MIN_CARGO_VERSION"; then
        warn "Found cargo $cargo_ver, but $MIN_CARGO_VERSION+ is recommended. Installing rustup's toolchain alongside it."
        need_rustup=true
    else
        log "Found cargo $cargo_ver — good enough."
    fi
fi

if [ "$need_rustup" = true ]; then
    if command -v rustup >/dev/null 2>&1; then
        log "rustup already present, updating stable toolchain..."
        rustup update stable
    else
        log "Installing Rust via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    fi
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
fi

command -v cargo >/dev/null 2>&1 || die "cargo still not on PATH after install — open a new shell and re-run this script."

# ---------------------------------------------------------------------------
# 4. Fetch the source (clone fresh, or pull if we've built this before)
# ---------------------------------------------------------------------------
if [ -d "$BUILD_DIR/.git" ]; then
    log "Updating existing checkout in $BUILD_DIR..."
    git -C "$BUILD_DIR" fetch --depth 1 origin "$BRANCH"
    git -C "$BUILD_DIR" reset --hard "origin/$BRANCH"
else
    log "Cloning $REPO_URL into $BUILD_DIR..."
    rm -rf "$BUILD_DIR"
    git clone --depth 1 -b "$BRANCH" "$REPO_URL" "$BUILD_DIR"
fi

# ---------------------------------------------------------------------------
# 5. Build
# ---------------------------------------------------------------------------
log "Building (this can take a few minutes the first time)..."
( cd "$BUILD_DIR" && cargo build --release )

BIN_SRC="$BUILD_DIR/target/release/universal-pm"
[ -x "$BIN_SRC" ] || die "Build finished but $BIN_SRC wasn't produced — check the cargo output above."

# ---------------------------------------------------------------------------
# 6. Install the binary + a desktop entry
# ---------------------------------------------------------------------------
if [ "$SYSTEM_INSTALL" = true ]; then
    BIN_DEST_DIR="/usr/local/bin"
    DESKTOP_DIR="/usr/local/share/applications"
    as_root install -Dm755 "$BIN_SRC" "$BIN_DEST_DIR/universal-pm"
else
    BIN_DEST_DIR="$HOME/.local/bin"
    DESKTOP_DIR="$HOME/.local/share/applications"
    mkdir -p "$BIN_DEST_DIR" "$DESKTOP_DIR"
    install -Dm755 "$BIN_SRC" "$BIN_DEST_DIR/universal-pm"
fi
log "Installed binary to $BIN_DEST_DIR/universal-pm"

DESKTOP_FILE="$DESKTOP_DIR/universal-pm.desktop"

desktop_entry="[Desktop Entry]
Type=Application
Name=Universal Package Manager
Comment=Cross-distro package manager frontend with Flatpak support
Exec=$BIN_DEST_DIR/universal-pm
Terminal=false
Categories=System;Settings;PackageManager;
"

if [ "$SYSTEM_INSTALL" = true ]; then
    as_root mkdir -p "$DESKTOP_DIR"
    printf '%s' "$desktop_entry" | as_root tee "$DESKTOP_FILE" >/dev/null
else
    mkdir -p "$DESKTOP_DIR"
    printf '%s' "$desktop_entry" > "$DESKTOP_FILE"
fi
log "Installed desktop entry to $DESKTOP_FILE"

# ---------------------------------------------------------------------------
# 7. PATH sanity check
# ---------------------------------------------------------------------------
if [ "$SYSTEM_INSTALL" = false ] && [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    warn "$HOME/.local/bin isn't on your PATH yet."
    warn 'Add this to your ~/.bashrc or ~/.profile, then restart your shell:'
    warn '  export PATH="$HOME/.local/bin:$PATH"'
fi

log "Done! Launch it from your app menu as 'Universal Package Manager', or run:"
echo "    $BIN_DEST_DIR/universal-pm"
