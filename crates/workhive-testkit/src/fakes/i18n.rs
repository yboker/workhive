//! Fake i18n adapter for testing.

use workhive_core::domain::Locale;
use workhive_core::ports::I18nPort;

/// Fake i18n that returns the key itself, making assertions easy.
///
/// In tests, you can simply assert that the output contains the expected key
/// without worrying about the actual translated text.
pub struct FakeI18n;

impl FakeI18n {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FakeI18n {
    fn default() -> Self {
        Self::new()
    }
}

impl I18nPort for FakeI18n {
    fn resolve(&self, _locale: Locale, key: &str) -> String {
        key.to_owned()
    }

    fn resolve_with_args(&self, _locale: Locale, key: &str, args: &[&str]) -> String {
        let mut result = key.to_owned();
        for (i, arg) in args.iter().enumerate() {
            result = result.replace(&format!("{{{i}}}"), arg);
        }
        result
    }
}
