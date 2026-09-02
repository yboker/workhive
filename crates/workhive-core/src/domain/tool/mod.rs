//! Tool catalog domain types.
//!
//! This module defines the structured pane-launch model and global tool catalog.
//! The canonical launch model replaces raw `command: String` in `PaneConfig`.
//!
//! ## Ownership boundary
//!
//! - **Global tool catalog** (`ToolCatalog`): owns reusable cross-project tools
//!   (claude, codex, lazygit, yazi, …).
//! - **Project layout**: may reference `Tool(ToolName)` for catalog-managed tools,
//!   `InlineCommand(CommandSpec)` for project-specific ad hoc commands, or
//!   `EmptyShell` for manual panes.
//!
//! Project layout MUST NOT be reduced to tool-name references only.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize, ser::SerializeMap};

// ── ToolName ─────────────────────────────────────────────────────────────────

/// Validated identifier for a tool in the catalog.
///
/// A `ToolName` is a short, lowercase, hyphen-allowed slug (e.g., `"claude"`,
/// `"lazygit"`, `"my-tool"`). It must be non-empty and contain only
/// `[a-z0-9_-]` characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolName(String);

impl ToolName {
    /// Create a `ToolName` from a raw string, validating format.
    ///
    /// Allowed characters: lowercase ASCII letters, digits, `_`, `-`.
    /// Must be non-empty.
    pub fn new(raw: impl Into<String>) -> Result<Self, ToolNameError> {
        let s = raw.into();
        if s.is_empty() {
            return Err(ToolNameError::Empty);
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(ToolNameError::InvalidChars(s));
        }
        Ok(Self(s))
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Errors produced when constructing a [`ToolName`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ToolNameError {
    #[error("tool name must not be empty")]
    Empty,

    #[error("tool name '{0}' contains invalid characters (only [a-z0-9_-] allowed)")]
    InvalidChars(String),
}

// ── CommandSpec ───────────────────────────────────────────────────────────────

/// Structured inline command — program + argument list.
///
/// Used for project-specific ad hoc commands that do not belong in the global
/// tool catalog. Each argument is stored separately; no shell-string splitting
/// is ever performed.
///
/// # Example
///
/// ```
/// use workhive_core::domain::CommandSpec;
/// let spec = CommandSpec::new("cargo", ["test", "--workspace"]);
/// assert_eq!(spec.program(), "cargo");
/// assert_eq!(spec.args(), &["test", "--workspace"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandSpec {
    /// Executable name or absolute path.
    pub program: String,
    /// Argument list (no shell expansion).
    pub args: Vec<String>,
}

impl CommandSpec {
    /// Build a `CommandSpec` from a program name and zero or more args.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// Return the program name.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Return the argument slice.
    pub fn args(&self) -> &[String] {
        &self.args
    }
}

impl fmt::Display for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.args.is_empty() {
            f.write_str(&self.program)
        } else {
            write!(f, "{} {}", self.program, self.args.join(" "))
        }
    }
}

// ── PaneLaunchRef ─────────────────────────────────────────────────────────────

/// Structured reference for how a pane should be launched.
///
/// This is the canonical launch model that replaces the legacy `command: String`
/// field on `PaneConfig`. It is an explicit, typed description of user intent —
/// no implicit coercions are allowed.
///
/// ## Variants
///
/// | Variant | Meaning |
/// |---------|---------|
/// | `EmptyShell` | Create the pane but execute nothing; leave it for manual use |
/// | `Tool(ToolName)` | Launch a tool resolved via the global tool catalog |
/// | `InlineCommand(CommandSpec)` | Launch a project-specific ad hoc command |
///
/// ## Non-negotiable rules
///
/// - `EmptyShell` is **not** an error path; it is a first-class user intent.
/// - `Tool` references MUST be resolved through the catalog before execution.
/// - `InlineCommand` is for project-local work that should NOT enter the catalog.
/// - Raw shell string hacks (`""` for EmptyShell) are forbidden.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneLaunchRef {
    /// Open the pane but execute no command — left for manual use.
    EmptyShell,
    /// Launch a tool resolved from the global catalog.
    ///
    /// Uses a named field (`name`) instead of a newtype tuple to remain
    /// compatible with TOML's internally-tagged enum serialization.
    Tool {
        /// Tool identifier in the catalog.
        name: ToolName,
    },
    /// Launch a project-specific ad hoc command.
    ///
    /// Uses a named field (`spec`) instead of a newtype tuple to remain
    /// compatible with TOML's internally-tagged enum serialization.
    InlineCommand {
        /// The structured command to run.
        spec: CommandSpec,
    },
}

impl PaneLaunchRef {
    /// Human-readable short name for display in pane borders and status bars.
    ///
    /// Returns the tool name for catalog tools, the program name for inline
    /// commands, and a generic label for empty shells. This is the canonical
    /// source for pane display titles — callers must not derive titles by
    /// inspecting variant fields directly.
    pub fn display_name(&self) -> &str {
        match self {
            Self::EmptyShell => "shell",
            Self::Tool { name } => name.as_str(),
            Self::InlineCommand { spec } => spec.program(),
        }
    }
}

impl fmt::Display for PaneLaunchRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyShell => f.write_str("<empty shell>"),
            Self::Tool { name } => write!(f, "tool:{name}"),
            Self::InlineCommand { spec } => write!(f, "cmd:{spec}"),
        }
    }
}

// ── ToolKind ──────────────────────────────────────────────────────────────────

/// Declared kind of a tool in the catalog.
///
/// `ToolKind` is the **authoritative metadata** for deriving a tool's baseline
/// `PaneKind` and capabilities at launch resolution time. It replaces
/// tool-name string matching as the capability derivation source.
///
/// Routing and orchestration logic must never inspect tool names directly —
/// they derive capabilities from `ToolKind` → `PaneKind` → `PaneCapability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// An AI coding agent that accepts and produces structured messages
    /// (e.g., claude, codex, opencode, aider).
    Agent,
    /// A full-screen TUI application (e.g., lazygit, yazi, htop).
    Tui,
    /// A general-purpose command-line tool or shell command.
    #[default]
    Cli,
}

impl ToolKind {
    /// Map this tool kind to the corresponding baseline `PaneKind`.
    ///
    /// This is the single authoritative mapping from catalog metadata to
    /// pane classification. It replaces tool-name string matching in
    /// `baseline_kind_for_tool`.
    pub fn to_pane_kind(self) -> super::pane::PaneKind {
        match self {
            Self::Agent => super::pane::PaneKind::Agent,
            Self::Tui => super::pane::PaneKind::GitUi,
            Self::Cli => super::pane::PaneKind::Terminal,
        }
    }
}

// ── ToolEntry / ToolCatalog ───────────────────────────────────────────────────

/// A single entry in the tool catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Unique tool identifier.
    pub name: ToolName,
    /// Human-readable description.
    pub description: String,
    /// Executable command (program + default args).
    pub command: CommandSpec,
    /// Declared tool kind — drives baseline `PaneKind` and capability derivation.
    ///
    /// `None` means the user did not explicitly set `kind` in their TOML config.
    /// In that case, [`ToolEntry::effective_kind`] applies legacy inference for
    /// well-known builtin tool names, defaulting to `Cli` for unknown tools.
    /// `Some(kind)` means the user explicitly chose a kind — inference is skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ToolKind>,
    /// Optional explicit process basenames this tool can present as.
    ///
    /// The pane runtime observation merge policy uses this list for
    /// **exact** basename matching against currently observed runtime
    /// facts (foreground command, process tree). It is the only way to
    /// teach the matcher about architecture-suffixed binaries
    /// (e.g. `codex-x86_64-apple-darwin`) or wrapper names without
    /// resorting to broad substring matching.
    ///
    /// Empty by default; when non-empty, [`ToolEntry::matches_process_name`]
    /// performs exact, case-sensitive comparisons. The legacy default
    /// (matching only against `command.program` basename) still
    /// applies when this list is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub process_names: Vec<String>,
    /// Whether the tool is currently enabled for use in layouts.
    pub enabled: bool,
}

impl ToolEntry {
    /// Create a new enabled tool entry with a specified kind.
    pub fn new(
        name: ToolName,
        description: impl Into<String>,
        command: CommandSpec,
        kind: ToolKind,
    ) -> Self {
        Self {
            name,
            description: description.into(),
            command,
            kind: Some(kind),
            process_names: Vec::new(),
            enabled: true,
        }
    }

    /// Return the effective `ToolKind` for this entry.
    ///
    /// - If the user explicitly set `kind` in config → use that.
    /// - If `kind` is `None` (legacy config, no `kind` field) → infer from
    ///   the tool name for well-known builtins, otherwise default to `Cli`.
    pub fn effective_kind(&self) -> ToolKind {
        match self.kind {
            Some(k) => k,
            None => infer_kind_for_known_tool(self.name.as_str()),
        }
    }

    /// Effective list of basenames this tool may present as at runtime.
    ///
    /// When `process_names` is non-empty, exactly that list is returned.
    /// Otherwise the matcher falls back to the basename of the
    /// configured `command.program` so simple tools don't need any
    /// extra config to recognise themselves. The fallback path NEVER
    /// engages substring matching; it is still an exact basename
    /// equality check.
    pub fn effective_process_names(&self) -> Vec<&str> {
        if !self.process_names.is_empty() {
            self.process_names.iter().map(String::as_str).collect()
        } else {
            program_basename(&self.command.program)
                .map(|s| vec![s])
                .unwrap_or_default()
        }
    }

    /// Whether the tool's effective process-name list contains a basename.
    ///
    /// Comparison is exact and case-sensitive (Unix process names are
    /// case-sensitive). Empty `basename` never matches.
    pub fn matches_process_name(&self, basename: &str) -> bool {
        if basename.is_empty() {
            return false;
        }
        self.effective_process_names()
            .into_iter()
            .any(|name| name == basename)
    }

    /// Validate that the tool entry has a well-formed configuration.
    ///
    /// Returns `None` if valid, or `Some(reason)` describing the problem.
    pub fn validate(&self) -> Option<String> {
        if self.command.program.is_empty() {
            return Some("program must not be empty".to_owned());
        }
        if self.command.program.contains('\0') {
            return Some("program must not contain null bytes".to_owned());
        }
        for (i, arg) in self.command.args.iter().enumerate() {
            if arg.contains('\0') {
                return Some(format!("argument {i} must not contain null bytes"));
            }
        }
        for (i, name) in self.process_names.iter().enumerate() {
            if name.is_empty() {
                return Some(format!("process_names[{i}] must not be empty"));
            }
            if name.contains('\0') {
                return Some(format!("process_names[{i}] must not contain null bytes"));
            }
            if name.contains('/') || name.contains('\\') {
                return Some(format!("process_names[{i}] must be a basename, not a path"));
            }
        }
        None
    }
}

/// Return the basename of a program string, or `None` when empty.
///
/// `program` may be a bare name (`codex`) or an absolute path
/// (`/usr/local/bin/codex`). Trailing slashes degrade to empty and
/// return `None` so callers don't accidentally accept malformed input.
fn program_basename(program: &str) -> Option<&str> {
    if program.is_empty() {
        return None;
    }
    let trimmed = program.trim_end_matches('/').trim_end_matches('\\');
    let last = trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())?;
    Some(last)
}

/// Infer `ToolKind` from a well-known builtin tool name.
///
/// This is a **backward-compatibility deserialization migration** that runs
/// for legacy config entries where `kind` was not explicitly set (`None`).
/// It is NOT runtime routing heuristic — once a user adds `kind = "agent"`
/// to their TOML, this function is never called for that entry.
fn infer_kind_for_known_tool(name: &str) -> ToolKind {
    match name {
        "claude" | "codex" | "opencode" | "aider" | "gemini" | "copilot" => ToolKind::Agent,
        "lazygit" | "yazi" | "htop" | "btop" | "tig" => ToolKind::Tui,
        _ => ToolKind::Cli,
    }
}

/// Global catalog of reusable cross-project tools.
///
/// The catalog is the authoritative source for all `Tool(ToolName)` references
/// in project layouts. Layout resolution MUST go through the catalog; it must
/// never resolve tool names by string matching alone.
///
/// The built-in defaults provide the standard workhive tool set. Users can
/// add, remove, enable, and disable entries via `workhive tool` subcommands.
#[derive(Debug, Clone, Default)]
pub struct ToolCatalog {
    entries: Vec<ToolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCatalogEntryConfig {
    description: String,
    command: CommandSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<ToolKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    process_names: Vec<String>,
    enabled: bool,
}

impl From<&ToolEntry> for ToolCatalogEntryConfig {
    fn from(entry: &ToolEntry) -> Self {
        Self {
            description: entry.description.clone(),
            command: entry.command.clone(),
            kind: entry.kind,
            process_names: entry.process_names.clone(),
            enabled: entry.enabled,
        }
    }
}

impl ToolCatalogEntryConfig {
    fn into_entry(self, name: ToolName) -> ToolEntry {
        ToolEntry {
            name,
            description: self.description,
            command: self.command,
            kind: self.kind,
            process_names: self.process_names,
            enabled: self.enabled,
        }
    }
}

impl Serialize for ToolCatalog {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for entry in &self.entries {
            map.serialize_entry(entry.name.as_str(), &ToolCatalogEntryConfig::from(entry))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ToolCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawCatalog {
            #[serde(default)]
            entries: Vec<ToolEntry>,
            #[serde(flatten)]
            named: BTreeMap<String, ToolCatalogEntryConfig>,
        }

        let raw = RawCatalog::deserialize(deserializer)?;
        let mut catalog = ToolCatalog::empty();

        for entry in raw.entries {
            catalog.add(entry).map_err(serde::de::Error::custom)?;
        }
        for (name, entry) in raw.named {
            let name = ToolName::new(name).map_err(serde::de::Error::custom)?;
            catalog
                .add(entry.into_entry(name))
                .map_err(serde::de::Error::custom)?;
        }

        Ok(catalog)
    }
}

impl ToolCatalog {
    /// Construct an empty catalog.
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Construct the built-in default catalog.
    ///
    /// Contains: `claude`, `codex`, `cursor-agent`, `opencode`,
    /// `lazygit`, `yazi`.
    ///
    /// Built-in tools provide explicit `process_names` patterns so the
    /// pane runtime observation merge policy can recognize their
    /// architecture-suffixed binaries (e.g. `codex-x86_64-apple-darwin`)
    /// or alias executables (e.g. Cursor Agent's `agent` symlink)
    /// without resorting to substring matching.
    pub fn builtin_defaults() -> Self {
        fn agent(
            name: &str,
            description: &str,
            command: CommandSpec,
            process_names: &[&str],
        ) -> ToolEntry {
            let mut entry = ToolEntry::new(
                ToolName::new(name).expect("builtin name is valid"),
                description,
                command,
                ToolKind::Agent,
            );
            entry.process_names = process_names.iter().map(|s| (*s).to_owned()).collect();
            entry
        }

        let defaults = vec![
            agent(
                "claude",
                "Anthropic Claude coding agent",
                CommandSpec::new("claude", ["--dangerously-skip-permissions"]),
                &["claude"],
            ),
            agent(
                "codex",
                "OpenAI Codex CLI coding agent",
                // Workhive-managed Codex panes run unattended next to other
                // managed agents (Claude uses `--dangerously-skip-permissions`,
                // Cursor Agent uses `--yolo`). The central catalog encodes
                // the same intentional unattended-local-agent mode for Codex
                // so all catalog-resolved launches (init wizard, tmux pane,
                // snapshot restore) inherit the flag from one place rather
                // than from scattered command strings.
                CommandSpec::new("codex", ["--dangerously-bypass-approvals-and-sandbox"]),
                &[
                    "codex",
                    "codex-x86_64-apple-darwin",
                    "codex-aarch64-apple-darwin",
                    "codex-x86_64-unknown-linux-gnu",
                    "codex-aarch64-unknown-linux-gnu",
                ],
            ),
            agent(
                "opencode",
                "OpenCode AI coding agent",
                CommandSpec::new("opencode", [] as [&str; 0]),
                &["opencode"],
            ),
            // Cursor Agent ships as two identical executables in
            // most installs: `cursor-agent` (canonical) and `agent`
            // (alias symlink). They share the same SHA-256 hash and
            // the same CLI help. Workhive must classify both
            // basenames as the SAME tool identity. The canonical
            // entry name is `cursor-agent`;
            // `agent` is modeled as an exact process-name alias, not
            // a separate provider. Provider attribution remains
            // unverified for Cursor Agent — `AiActivityProvider`
            // intentionally does NOT map `cursor-agent` to any
            // supported provider.
            agent(
                "cursor-agent",
                "Cursor AI coding agent",
                CommandSpec::new("cursor-agent", ["--yolo"]),
                &["cursor-agent", "agent"],
            ),
            ToolEntry::new(
                ToolName::new("lazygit").expect("builtin name is valid"),
                "Terminal UI for git",
                CommandSpec::new("lazygit", [] as [&str; 0]),
                ToolKind::Tui,
            ),
            ToolEntry::new(
                ToolName::new("yazi").expect("builtin name is valid"),
                "Terminal file manager",
                CommandSpec::new("yazi", [] as [&str; 0]),
                ToolKind::Tui,
            ),
        ];
        Self { entries: defaults }
    }

    /// Find an enabled tool whose effective process names match the
    /// given basename exactly.
    ///
    /// Used by the pane runtime observation merge policy. Returns the
    /// first match in insertion order; ties are not expected because
    /// process names should be unique per tool.
    pub fn match_process_name(&self, basename: &str) -> Option<&ToolEntry> {
        if basename.is_empty() {
            return None;
        }
        self.enabled_iter()
            .find(|entry| entry.matches_process_name(basename))
    }

    /// Look up a tool by name. Returns `None` if not found or disabled.
    pub fn resolve(&self, name: &ToolName) -> Option<&ToolEntry> {
        self.entries.iter().find(|e| &e.name == name && e.enabled)
    }

    /// Look up a tool by name regardless of enabled state.
    pub fn find(&self, name: &ToolName) -> Option<&ToolEntry> {
        self.entries.iter().find(|e| &e.name == name)
    }

    /// Add a new entry. Returns an error if a tool with the same name already exists.
    pub fn add(&mut self, entry: ToolEntry) -> Result<(), CatalogError> {
        if self.entries.iter().any(|e| e.name == entry.name) {
            return Err(CatalogError::AlreadyExists(entry.name));
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Remove a tool by name. Returns an error if not found.
    pub fn remove(&mut self, name: &ToolName) -> Result<(), CatalogError> {
        let pos = self
            .entries
            .iter()
            .position(|e| &e.name == name)
            .ok_or_else(|| CatalogError::NotFound(name.clone()))?;
        self.entries.remove(pos);
        Ok(())
    }

    /// Enable a tool. Returns an error if not found.
    pub fn enable(&mut self, name: &ToolName) -> Result<(), CatalogError> {
        self.entries
            .iter_mut()
            .find(|e| &e.name == name)
            .ok_or_else(|| CatalogError::NotFound(name.clone()))
            .map(|e| e.enabled = true)
    }

    /// Disable a tool. Returns an error if not found.
    pub fn disable(&mut self, name: &ToolName) -> Result<(), CatalogError> {
        self.entries
            .iter_mut()
            .find(|e| &e.name == name)
            .ok_or_else(|| CatalogError::NotFound(name.clone()))
            .map(|e| e.enabled = false)
    }

    /// Iterate over all entries (enabled and disabled).
    pub fn iter(&self) -> impl Iterator<Item = &ToolEntry> {
        self.entries.iter()
    }

    /// Iterate over enabled entries only.
    pub fn enabled_iter(&self) -> impl Iterator<Item = &ToolEntry> {
        self.entries.iter().filter(|e| e.enabled)
    }

    /// Return the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if the catalog has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Errors produced by catalog mutation operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("tool '{0}' already exists in the catalog")]
    AlreadyExists(ToolName),

    #[error("tool '{0}' not found in the catalog")]
    NotFound(ToolName),
}

mod resolved;

pub use resolved::*;

#[cfg(test)]
#[path = "../tool_tests.rs"]
mod tests;
