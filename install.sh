#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# workhive installer
# Usage: curl -fsSL https://raw.githubusercontent.com/yboker/workhive/main/install.sh | bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

# ──────────────────────────────────────────────────
# Constants
# ──────────────────────────────────────────────────

REPO_OWNER="yboker"
REPO_NAME="workhive"
REPO_URL="https://github.com/${REPO_OWNER}/${REPO_NAME}"
DEFAULT_PREFIX="${HOME}/.local"
VERSION=""
UNINSTALL=false
PREFIX=""

# ──────────────────────────────────────────────────
# Helpers
# ──────────────────────────────────────────────────

info()  { printf "\033[1;34m==>\033[0m %s\n" "$*"; }
warn()  { printf "\033[1;33mWARN:\033[0m %s\n" "$*" >&2; }
error() { printf "\033[1;31mERROR:\033[0m %s\n" "$*" >&2; }
success() { printf "\033[1;32m==>\033[0m %s\n" "$*"; }

# ──────────────────────────────────────────────────
# Cleanup
# ──────────────────────────────────────────────────

TMPDIR_INSTALL=""

cleanup() {
    if [[ -n "$TMPDIR_INSTALL" && -d "$TMPDIR_INSTALL" ]]; then
        rm -rf "$TMPDIR_INSTALL"
    fi
}

trap cleanup EXIT

# ──────────────────────────────────────────────────
# Platform detection
# ──────────────────────────────────────────────────

detect_platform() {
    local os
    os="$(uname -s)"
    case "$os" in
        Darwin)  echo "macos" ;;
        Linux)
            if grep -qi microsoft /proc/version 2>/dev/null; then
                echo "wsl2"
            else
                echo "linux"
            fi
            ;;
        MINGW*|MSYS*|CYGWIN*)
            echo "windows-git-bash"
            ;;
        *)
            error "Unsupported platform: $os"
            exit 1
            ;;
    esac
}

# ──────────────────────────────────────────────────
# Dependency checks
# ──────────────────────────────────────────────────

check_dependencies() {
    local missing=()

    if ! command -v git &>/dev/null; then
        missing+=("git")
    fi
    if ! command -v tmux &>/dev/null; then
        missing+=("tmux")
    fi
    if ! command -v yq &>/dev/null; then
        missing+=("yq")
    fi

    if [[ ${#missing[@]} -gt 0 ]]; then
        warn "Missing required dependencies: ${missing[*]}"
        echo ""
        echo "Installation guide:"
        local platform
        platform=$(detect_platform)
        for dep in "${missing[@]}"; do
            case "$dep" in
                git)
                    case "$platform" in
                        macos) echo "  brew install git" ;;
                        linux|wsl2) echo "  sudo apt install git  # or: sudo yum install git" ;;
                    esac
                    ;;
                tmux)
                    case "$platform" in
                        macos) echo "  brew install tmux" ;;
                        linux|wsl2) echo "  sudo apt install tmux" ;;
                    esac
                    ;;
                yq)
                    case "$platform" in
                        macos) echo "  brew install yq" ;;
                        linux|wsl2) echo "  # See: https://github.com/mikefarah/yq#install" ;;
                    esac
                    ;;
            esac
        done
        echo ""
        warn "Install the above dependencies, then re-run this script."
        exit 1
    fi
}

# ──────────────────────────────────────────────────
# Homebrew conflict detection
# ──────────────────────────────────────────────────

check_homebrew_conflict() {
    if command -v brew &>/dev/null; then
        if brew list --formula 2>/dev/null | grep -q "^workhive$"; then
            error "workhive is already installed via Homebrew."
            echo "  Use: brew upgrade workhive"
            echo "  Or uninstall first: brew uninstall workhive"
            exit 1
        fi
    fi
}

# ──────────────────────────────────────────────────
# Download source
# ──────────────────────────────────────────────────

download_source() {
    local dest="$1"

    # Try downloading a specific version tag
    if [[ -n "$VERSION" ]]; then
        local tarball_url="${REPO_URL}/archive/refs/tags/${VERSION}.tar.gz"
        info "Downloading workhive ${VERSION}..."
        if curl -fsSL "$tarball_url" | tar xz -C "$dest" --strip-components=1; then
            return 0
        fi
        error "Failed to download version ${VERSION}. Check if the tag exists."
        exit 1
    fi

    # Default: download latest from main branch
    local tarball_url="${REPO_URL}/archive/refs/heads/main.tar.gz"
    info "Downloading workhive (latest)..."
    if curl -fsSL "$tarball_url" | tar xz -C "$dest" --strip-components=1; then
        return 0
    fi

    # Fallback: git clone
    info "Tarball download failed, falling back to git clone..."
    if git clone --depth=1 "${REPO_URL}.git" "$dest"; then
        return 0
    fi

    error "Failed to download workhive. Check your network connection."
    exit 1
}

# ──────────────────────────────────────────────────
# Install
# ──────────────────────────────────────────────────

do_install() {
    local prefix="$1"

    check_dependencies
    check_homebrew_conflict

    # Create temp directory
    TMPDIR_INSTALL="$(mktemp -d "${TMPDIR:-/tmp}/workhive-install.XXXXXX")"
    local src_dir="$TMPDIR_INSTALL/src"
    mkdir -p "$src_dir"

    # Download
    download_source "$src_dir"

    # Verify source
    if [[ ! -f "$src_dir/bin/workhive" ]]; then
        error "Downloaded source is invalid (bin/workhive not found)"
        exit 1
    fi

    # Create install directories
    local bin_dir="${prefix}/bin"
    local lib_dir="${prefix}/lib/workhive"
    local share_dir="${prefix}/share/workhive"

    mkdir -p "$bin_dir" "$lib_dir" "$share_dir"

    # Install lib/ (core libraries)
    info "Installing libraries to ${lib_dir}..."
    if [[ -d "${lib_dir}/lib" ]]; then
        rm -rf "${lib_dir:?}/lib"
    fi
    cp -R "$src_dir/lib" "${lib_dir}/lib"

    # Install shared resources
    if [[ -d "$src_dir/configs" ]]; then
        cp -R "$src_dir/configs" "${share_dir}/configs" 2>/dev/null || true
    fi
    if [[ -d "$src_dir/examples" ]]; then
        cp -R "$src_dir/examples" "${share_dir}/examples" 2>/dev/null || true
    fi

    # Install completions
    if [[ -d "$src_dir/completions" ]]; then
        cp -R "$src_dir/completions" "${share_dir}/completions" 2>/dev/null || true
    fi

    # Install entry script (atomic write via .new + mv)
    local entry_new="${bin_dir}/workhive.new"
    cp "$src_dir/bin/workhive" "$entry_new"

    # Patch ROOT_DIR and LIB_DIR paths in the entry script
    # Use awk for robustness (avoids sed special char issues in paths)
    awk -v dir="$lib_dir" '/^ROOT_DIR=/{print "ROOT_DIR=\"" dir "\""; next} {print}' \
        "$entry_new" > "${entry_new}.tmp" && mv "${entry_new}.tmp" "$entry_new"
    awk -v dir="${lib_dir}/lib" '/^export LIB_DIR=/{print "export LIB_DIR=\"" dir "\""; next} {print}' \
        "$entry_new" > "${entry_new}.tmp" && mv "${entry_new}.tmp" "$entry_new"

    chmod +x "$entry_new"
    mv "$entry_new" "${bin_dir}/workhive"

    # Install shell completions to standard locations
    install_completions "$src_dir" "$prefix"

    # Verify installation
    if "${bin_dir}/workhive" --version &>/dev/null; then
        success "workhive installed successfully!"
        echo ""
        "${bin_dir}/workhive" --version
    else
        warn "Installation completed but verification failed."
        warn "Try running: ${bin_dir}/workhive --version"
    fi

    # Check PATH
    check_path "$bin_dir"

    echo ""
    info "Getting started:"
    echo "  cd your-project"
    echo "  workhive init"
    echo "  workhive open feature-x"
    echo ""
    info "Uninstall:"
    echo "  curl -fsSL https://raw.githubusercontent.com/${REPO_OWNER}/${REPO_NAME}/main/install.sh | bash -s -- --uninstall --prefix ${prefix}"
}

# ──────────────────────────────────────────────────
# Install shell completions
# ──────────────────────────────────────────────────

install_completions() {
    local src_dir="$1"
    local prefix="$2"

    # Bash completions
    if [[ -f "$src_dir/completions/workhive.bash" ]]; then
        local bash_comp_dir=""
        if [[ -d "${prefix}/share/bash-completion/completions" ]]; then
            bash_comp_dir="${prefix}/share/bash-completion/completions"
        elif [[ -d "/usr/local/share/bash-completion/completions" ]]; then
            bash_comp_dir="/usr/local/share/bash-completion/completions"
        elif [[ -d "${HOME}/.local/share/bash-completion/completions" ]]; then
            bash_comp_dir="${HOME}/.local/share/bash-completion/completions"
        fi
        if [[ -n "$bash_comp_dir" ]]; then
            cp "$src_dir/completions/workhive.bash" "${bash_comp_dir}/workhive" 2>/dev/null || true
        fi
    fi

    # Zsh completions
    if [[ -f "$src_dir/completions/workhive.zsh" ]]; then
        local zsh_comp_dir=""
        if [[ -d "${prefix}/share/zsh/site-functions" ]]; then
            zsh_comp_dir="${prefix}/share/zsh/site-functions"
        elif [[ -d "/usr/local/share/zsh/site-functions" ]]; then
            zsh_comp_dir="/usr/local/share/zsh/site-functions"
        elif [[ -d "${HOME}/.zsh/completions" ]]; then
            zsh_comp_dir="${HOME}/.zsh/completions"
        fi
        if [[ -n "$zsh_comp_dir" ]]; then
            cp "$src_dir/completions/workhive.zsh" "${zsh_comp_dir}/_workhive" 2>/dev/null || true
        fi
    fi

    # Fish completions
    if [[ -f "$src_dir/completions/workhive.fish" ]]; then
        local fish_comp_dir=""
        if [[ -d "${prefix}/share/fish/vendor_completions.d" ]]; then
            fish_comp_dir="${prefix}/share/fish/vendor_completions.d"
        elif [[ -d "/usr/local/share/fish/vendor_completions.d" ]]; then
            fish_comp_dir="/usr/local/share/fish/vendor_completions.d"
        elif [[ -d "${HOME}/.config/fish/completions" ]]; then
            fish_comp_dir="${HOME}/.config/fish/completions"
        fi
        if [[ -n "$fish_comp_dir" ]]; then
            cp "$src_dir/completions/workhive.fish" "${fish_comp_dir}/workhive.fish" 2>/dev/null || true
        fi
    fi
}

# ──────────────────────────────────────────────────
# Check PATH
# ──────────────────────────────────────────────────

check_path() {
    local bin_dir="$1"

    case ":${PATH}:" in
        *":${bin_dir}:"*)
            # Already in PATH
            ;;
        *)
            warn "${bin_dir} is not in your PATH."
            echo ""
            echo "Add it to your shell profile:"
            local shell_name
            shell_name="$(basename "${SHELL:-/bin/bash}")"
            case "$shell_name" in
                zsh)
                    echo "  echo 'export PATH=\"${bin_dir}:\$PATH\"' >> ~/.zshrc"
                    echo "  source ~/.zshrc"
                    ;;
                bash)
                    echo "  echo 'export PATH=\"${bin_dir}:\$PATH\"' >> ~/.bashrc"
                    echo "  source ~/.bashrc"
                    ;;
                fish)
                    echo "  fish_add_path ${bin_dir}"
                    ;;
                *)
                    echo "  export PATH=\"${bin_dir}:\$PATH\""
                    ;;
            esac
            ;;
    esac
}

# ──────────────────────────────────────────────────
# Uninstall
# ──────────────────────────────────────────────────

do_uninstall() {
    local prefix="$1"

    local bin_dir="${prefix}/bin"
    local lib_dir="${prefix}/lib/workhive"
    local share_dir="${prefix}/share/workhive"

    info "Uninstalling workhive from ${prefix}..."

    local removed=false

    if [[ -f "${bin_dir}/workhive" ]]; then
        rm -f "${bin_dir}/workhive"
        success "Removed ${bin_dir}/workhive"
        removed=true
    fi

    if [[ -d "$lib_dir" ]]; then
        rm -rf "$lib_dir"
        success "Removed ${lib_dir}"
        removed=true
    fi

    if [[ -d "$share_dir" ]]; then
        rm -rf "$share_dir"
        success "Removed ${share_dir}"
        removed=true
    fi

    # Remove completions (only from prefix-relative paths)
    local comp_files=(
        "${prefix}/share/bash-completion/completions/workhive"
        "${HOME}/.local/share/bash-completion/completions/workhive"
        "${prefix}/share/zsh/site-functions/_workhive"
        "${HOME}/.zsh/completions/_workhive"
        "${prefix}/share/fish/vendor_completions.d/workhive.fish"
        "${HOME}/.config/fish/completions/workhive.fish"
    )
    for f in "${comp_files[@]}"; do
        if [[ -f "$f" ]]; then
            rm -f "$f"
            success "Removed $f"
            removed=true
        fi
    done

    if [[ "$removed" == true ]]; then
        success "workhive uninstalled successfully."
    else
        warn "workhive not found at ${prefix}. Nothing to uninstall."
    fi
}

# ──────────────────────────────────────────────────
# Usage
# ──────────────────────────────────────────────────

show_help() {
    cat <<'EOF'
workhive installer

Usage:
  curl -fsSL https://raw.githubusercontent.com/yboker/workhive/main/install.sh | bash

Options:
  --prefix <dir>    Install prefix (default: ~/.local)
  --version <tag>   Install specific version (e.g., v0.2.0)
  --uninstall       Uninstall workhive
  --help            Show this help

Examples:
  # Install latest
  curl -fsSL ... | bash

  # Install specific version
  curl -fsSL ... | bash -s -- --version v0.2.0

  # Install to /usr/local
  curl -fsSL ... | bash -s -- --prefix /usr/local

  # Uninstall
  curl -fsSL ... | bash -s -- --uninstall
EOF
}

# ──────────────────────────────────────────────────
# Main (wrapped to prevent partial download execution)
# ──────────────────────────────────────────────────

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --prefix)
                if [[ $# -lt 2 || "${2:-}" == --* ]]; then
                    error "--prefix requires a directory argument"
                    exit 1
                fi
                PREFIX="$2"
                shift 2
                ;;
            --version)
                if [[ $# -lt 2 || "${2:-}" == --* ]]; then
                    error "--version requires a version tag argument"
                    exit 1
                fi
                VERSION="$2"
                shift 2
                ;;
            --uninstall)
                UNINSTALL=true
                shift
                ;;
            --help|-h)
                show_help
                exit 0
                ;;
            *)
                error "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done

    # Set default prefix
    if [[ -z "$PREFIX" ]]; then
        PREFIX="$DEFAULT_PREFIX"
    fi

    echo ""
    echo "  ╦ ╦┌─┐┬─┐┬┌─╦ ╦┬┬  ┬┌─┐"
    echo "  ║║║│ │├┬┘├┴┐╠═╣│└┐┌┘├┤ "
    echo "  ╚╩╝└─┘┴└─┴ ┴╩ ╩┴ └┘ └─┘"
    echo ""

    if [[ "$UNINSTALL" == true ]]; then
        do_uninstall "$PREFIX"
    else
        do_install "$PREFIX"
    fi
}

main "$@"
