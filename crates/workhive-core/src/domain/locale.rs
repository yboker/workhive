use std::fmt;

use serde::{Deserialize, Serialize};

/// Supported locales for user-facing messages.
///
/// Default is English. Can be overridden at global or project level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Locale {
    /// English (default).
    #[default]
    #[serde(rename = "en")]
    En,
    /// Simplified Chinese.
    #[serde(rename = "zh-CN")]
    ZhCn,
}

impl Locale {
    /// Parse a locale string (case-insensitive).
    ///
    /// Returns `None` for unrecognized values.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "en" | "english" => Some(Self::En),
            "zh-cn" | "zh_cn" | "chinese" => Some(Self::ZhCn),
            _ => None,
        }
    }

    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhCn => "zh-CN",
        }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_english() {
        assert_eq!(Locale::default(), Locale::En);
    }

    #[test]
    fn display() {
        assert_eq!(Locale::En.to_string(), "en");
        assert_eq!(Locale::ZhCn.to_string(), "zh-CN");
    }

    #[test]
    fn from_str_loose_variants() {
        assert_eq!(Locale::from_str_loose("en"), Some(Locale::En));
        assert_eq!(Locale::from_str_loose("English"), Some(Locale::En));
        assert_eq!(Locale::from_str_loose("zh-cn"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_str_loose("zh_cn"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_str_loose("ZH-CN"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_str_loose("fr"), None);
    }

    #[test]
    fn serde_roundtrip() {
        let json = serde_json::to_string(&Locale::ZhCn).expect("serialize");
        assert_eq!(json, "\"zh-CN\"");
        let parsed: Locale = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, Locale::ZhCn);
    }
}
