#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/infra/platform/detector.sh & path.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/platform/path.sh"

test_suite "lib/infra/platform/detector.sh"

# ──────────────────────────────────────────────────
# platform_detect
# ──────────────────────────────────────────────────

test_case "platform_detect returns a known platform"
platform=$(platform_detect)
assert_not_empty "$platform" "platform should not be empty"

test_case "platform_detect result is cacheable via env var"
export WORKHIVE_PLATFORM="test-platform"
result=$(platform_detect)
assert_eq "test-platform" "$result"
unset WORKHIVE_PLATFORM

test_case "platform_detect returns valid value on this machine"
result=$(platform_detect)
# On macOS it should be macos
if [[ "$(uname -s)" == "Darwin" ]]; then
    assert_eq "macos" "$result"
elif [[ "$(uname -s)" == "Linux" ]]; then
    # Could be linux or wsl2
    assert_contains "linux wsl2" "$result"
fi

# ──────────────────────────────────────────────────
# platform_is_supported
# ──────────────────────────────────────────────────

test_case "platform_is_supported returns 0 for macos"
platform_is_supported "macos"
assert_eq "0" "$?"

test_case "platform_is_supported returns 0 for linux"
platform_is_supported "linux"
assert_eq "0" "$?"

test_case "platform_is_supported returns 0 for wsl2"
platform_is_supported "wsl2"
assert_eq "0" "$?"

test_case "platform_is_supported returns 0 for windows-git-bash"
platform_is_supported "windows-git-bash"
assert_eq "0" "$?"

test_case "platform_is_supported returns 1 for unknown"
platform_is_supported "solaris" || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# platform_get_name
# ──────────────────────────────────────────────────

test_case "platform_get_name returns macOS for macos"
result=$(platform_get_name "macos")
assert_eq "macOS" "$result"

test_case "platform_get_name returns Linux for linux"
result=$(platform_get_name "linux")
assert_eq "Linux" "$result"

test_case "platform_get_name returns WSL2 for wsl2"
result=$(platform_get_name "wsl2")
assert_eq "Windows Subsystem for Linux 2" "$result"

test_case "platform_get_name returns Git Bash for windows-git-bash"
result=$(platform_get_name "windows-git-bash")
assert_eq "Windows (Git Bash)" "$result"

test_case "platform_get_name returns Unknown for unrecognized"
result=$(platform_get_name "freebsd")
assert_eq "Unknown Platform" "$result"

# ──────────────────────────────────────────────────
# platform_supports_feature
# ──────────────────────────────────────────────────

test_case "platform_supports_feature tmux on macos"
platform_supports_feature "tmux" "macos"
assert_eq "0" "$?"

test_case "platform_supports_feature tmux on linux"
platform_supports_feature "tmux" "linux"
assert_eq "0" "$?"

test_case "platform_supports_feature tmux on wsl2"
platform_supports_feature "tmux" "wsl2"
assert_eq "0" "$?"

test_case "platform_supports_feature tmux on windows-git-bash fails"
platform_supports_feature "tmux" "windows-git-bash" || rc=$?
assert_eq "1" "${rc:-0}"

test_case "platform_supports_feature screen on all platforms"
platform_supports_feature "screen" "macos"
assert_eq "0" "$?" "screen on macos"

test_case "platform_supports_feature native-symlinks on macos"
platform_supports_feature "native-symlinks" "macos"
assert_eq "0" "$?"

test_case "platform_supports_feature native-symlinks fails on windows-git-bash"
platform_supports_feature "native-symlinks" "windows-git-bash" || rc=$?
assert_eq "1" "${rc:-0}"

test_case "platform_supports_feature unknown feature fails"
platform_supports_feature "quantum-computing" "macos" || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# path.sh tests
# ──────────────────────────────────────────────────

test_suite "lib/infra/platform/path.sh"

test_case "path_join two components"
result=$(path_join "/home/user" "project")
assert_eq "/home/user/project" "$result"

test_case "path_join strips trailing/leading slashes"
result=$(path_join "/home/user/" "/project")
assert_eq "/home/user/project" "$result"

test_case "path_join multiple components"
result=$(path_join "/home" "user" "project" "src")
assert_eq "/home/user/project/src" "$result"

test_case "path_join single component"
result=$(path_join "/home/user")
assert_eq "/home/user" "$result"

test_case "path_absolute with absolute path"
result=$(path_absolute "/usr/bin/bash")
assert_eq "/usr/bin/bash" "$result"

test_case "path_absolute with relative path"
result=$(path_absolute "relative/path")
assert_contains "$result" "relative/path"
# Should start with /
assert_eq "/" "${result:0:1}" "should be absolute"

test_case "path_normalize Unix path unchanged on Unix"
result=$(path_normalize "/home/user/project" "macos")
assert_eq "/home/user/project" "$result"

test_case "path_to_unix on Unix returns same path"
result=$(path_to_unix "/home/user/project" "linux")
assert_eq "/home/user/project" "$result"

test_report
