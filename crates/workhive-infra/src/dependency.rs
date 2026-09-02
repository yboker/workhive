//! System dependency checker — verifies git/tmux availability and versions.
//!
//! Uses `std::process::Command` to probe the system. Platform-aware install
//! hints are provided for macOS, Linux (apt/dnf/pacman), and Windows (winget/scoop).

use std::process::Command;

use workhive_core::ports::{DependencyKind, DependencyPort, DependencyStatus};

const GIT_MIN_VERSION: &str = "2.20.0";
const TMUX_MIN_VERSION: &str = "3.0";

/// Real system dependency checker.
pub struct SystemDependencyChecker;

impl SystemDependencyChecker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemDependencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyPort for SystemDependencyChecker {
    fn check_all(&self) -> Vec<DependencyStatus> {
        vec![
            self.check(DependencyKind::Git),
            self.check(DependencyKind::Tmux),
        ]
    }

    fn check(&self, kind: DependencyKind) -> DependencyStatus {
        match kind {
            DependencyKind::Git => {
                check_command("git", &["--version"], "git version ", GIT_MIN_VERSION, kind)
            }
            DependencyKind::Tmux => check_command("tmux", &["-V"], "tmux ", TMUX_MIN_VERSION, kind),
        }
    }
}

/// Probe a system command, parse its version, and compare against a minimum.
fn check_command(
    binary: &str,
    args: &[&str],
    version_prefix: &str,
    min_version: &str,
    kind: DependencyKind,
) -> DependencyStatus {
    let output = Command::new(binary).args(args).output();

    let output = match output {
        Ok(o) => o,
        Err(_) => {
            tracing::debug!(binary, "dependency not found on PATH");
            return DependencyStatus::Missing { kind };
        }
    };

    if !output.status.success() {
        tracing::debug!(binary, code = ?output.status.code(), "version command failed");
        return DependencyStatus::Missing { kind };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = extract_version(&stdout, version_prefix);

    match version {
        Some(v) => {
            if version_satisfies(&v, min_version) {
                tracing::debug!(binary, version = %v, "dependency satisfied");
                DependencyStatus::Satisfied { kind, version: v }
            } else {
                tracing::warn!(binary, found = %v, minimum = min_version, "dependency version too low");
                DependencyStatus::VersionTooLow {
                    kind,
                    found: v,
                    minimum: min_version.to_owned(),
                }
            }
        }
        None => {
            tracing::warn!(binary, raw_output = %stdout.trim(), "could not parse version");
            DependencyStatus::Missing { kind }
        }
    }
}

/// Extract a semver-like version string from command output.
///
/// Handles formats like:
/// - `git version 2.43.0` → `2.43.0`
/// - `tmux 3.4` → `3.4`
/// - `tmux next-3.5` → `3.5`
fn extract_version(output: &str, prefix: &str) -> Option<String> {
    let line = output.lines().next()?;
    let after = line.strip_prefix(prefix).unwrap_or(line);

    let version_part = after
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>();

    if version_part.is_empty() {
        None
    } else {
        Some(version_part)
    }
}

/// Simple numeric version comparison (major.minor.patch).
///
/// Compares segment-by-segment: `2.43.0 >= 2.20.0` → true.
fn version_satisfies(found: &str, minimum: &str) -> bool {
    let found_parts: Vec<u32> = found.split('.').filter_map(|s| s.parse().ok()).collect();
    let min_parts: Vec<u32> = minimum.split('.').filter_map(|s| s.parse().ok()).collect();

    let max_len = found_parts.len().max(min_parts.len());
    for i in 0..max_len {
        let f = found_parts.get(i).copied().unwrap_or(0);
        let m = min_parts.get(i).copied().unwrap_or(0);
        if f > m {
            return true;
        }
        if f < m {
            return false;
        }
    }
    true // equal
}

/// Detected OS platform for install hint routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    /// Detect the current platform at runtime.
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Platform-specific install hint for a dependency.
///
/// Returns only the command relevant to the detected OS, not all platforms.
pub fn install_hint(kind: DependencyKind) -> String {
    install_hint_for(kind, Platform::detect())
}

/// Install hint for a specific platform (testable).
pub fn install_hint_for(kind: DependencyKind, platform: Platform) -> String {
    match (kind, platform) {
        (DependencyKind::Git, Platform::MacOs) => "    brew install git".into(),
        (DependencyKind::Git, Platform::Linux) => "    sudo apt install git  # Debian/Ubuntu\n    \
             sudo dnf install git  # Fedora\n    \
             sudo pacman -S git    # Arch"
            .into(),
        (DependencyKind::Git, Platform::Windows) => {
            "    winget install Git.Git  # or: scoop install git".into()
        }
        (DependencyKind::Tmux, Platform::MacOs) => "    brew install tmux".into(),
        (DependencyKind::Tmux, Platform::Linux) => {
            "    sudo apt install tmux  # Debian/Ubuntu\n    \
             sudo dnf install tmux  # Fedora\n    \
             sudo pacman -S tmux    # Arch"
                .into()
        }
        (DependencyKind::Tmux, Platform::Windows) => {
            "    WSL required — install tmux inside WSL:\n    \
             wsl --install && wsl sudo apt install tmux"
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_git_version() {
        assert_eq!(
            extract_version("git version 2.43.0", "git version "),
            Some("2.43.0".to_owned())
        );
    }

    #[test]
    fn extract_git_version_with_extra() {
        assert_eq!(
            extract_version("git version 2.39.5 (Apple Git-154)", "git version "),
            Some("2.39.5".to_owned())
        );
    }

    #[test]
    fn extract_tmux_version() {
        assert_eq!(extract_version("tmux 3.4", "tmux "), Some("3.4".to_owned()));
    }

    #[test]
    fn extract_tmux_next_version() {
        assert_eq!(
            extract_version("tmux next-3.5", "tmux "),
            Some("3.5".to_owned())
        );
    }

    #[test]
    fn version_comparison() {
        assert!(version_satisfies("2.43.0", "2.20.0"));
        assert!(version_satisfies("3.4", "3.0"));
        assert!(version_satisfies("2.20.0", "2.20.0"));
        assert!(!version_satisfies("2.19.0", "2.20.0"));
        assert!(!version_satisfies("1.8", "2.0"));
        assert!(version_satisfies("3.5", "3.0"));
    }

    #[test]
    fn system_check_git_exists() {
        let checker = SystemDependencyChecker::new();
        let status = checker.check(DependencyKind::Git);
        // git should be present on any dev machine
        assert!(status.is_ok(), "git not found on system: {status:?}");
    }

    #[test]
    fn install_hint_macos_returns_brew() {
        let hint = install_hint_for(DependencyKind::Git, Platform::MacOs);
        assert!(hint.contains("brew install git"), "expected brew: {hint}");
        assert!(!hint.contains("apt"), "macos hint should not mention apt");
    }

    #[test]
    fn install_hint_linux_returns_apt_and_dnf() {
        let hint = install_hint_for(DependencyKind::Tmux, Platform::Linux);
        assert!(hint.contains("apt install tmux"), "expected apt: {hint}");
        assert!(hint.contains("dnf install tmux"), "expected dnf: {hint}");
    }

    #[test]
    fn install_hint_windows_tmux_mentions_wsl() {
        let hint = install_hint_for(DependencyKind::Tmux, Platform::Windows);
        assert!(hint.contains("WSL"), "expected WSL: {hint}");
    }
}
