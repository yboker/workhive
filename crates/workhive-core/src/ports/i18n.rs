//! I18n port — abstracts localized message resolution.

use crate::domain::Locale;

/// Abstraction over internationalized message resolution.
///
/// All user-facing text in workhive goes through this trait.
/// Implementations provide static message catalogs or dynamic resolution.
pub trait I18nPort {
    /// Resolve a message key for the given locale.
    ///
    /// Returns the localized string, or falls back to English if the key
    /// is missing in the requested locale.
    fn resolve(&self, locale: Locale, key: &str) -> String;

    /// Resolve a message key with format arguments.
    ///
    /// The `args` slice provides positional replacements for `{0}`, `{1}`, etc.
    fn resolve_with_args(&self, locale: Locale, key: &str, args: &[&str]) -> String;
}
