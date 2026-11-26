//! Optional native browser locale. Not part of the frozen transfer v1 wire.
use serde::{Deserialize, Serialize};

pub const BROWSER_LANGUAGES: &[&str] = &[
    "zh-CN", "zh-TW", "en-US", "en-GB", "ja-JP", "ko-KR", "de-DE", "fr-FR", "es-ES", "pt-BR",
];
pub const BROWSER_TIMEZONES: &[&str] = &[
    "UTC",
    "Asia/Shanghai",
    "Asia/Hong_Kong",
    "Asia/Taipei",
    "Asia/Tokyo",
    "Asia/Singapore",
    "America/New_York",
    "America/Chicago",
    "America/Los_Angeles",
    "Europe/London",
    "Europe/Berlin",
    "Australia/Sydney",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserLocale {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "language"
    )]
    pub language: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "timezone"
    )]
    pub timezone: Option<String>,
}

fn known(value: &str, choices: &[&str]) -> Option<String> {
    choices
        .iter()
        .find(|item| item.eq_ignore_ascii_case(value.trim()))
        .map(|s| (*s).to_owned())
}
fn language<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().and_then(|s| known(s, BROWSER_LANGUAGES)))
}
fn timezone<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().and_then(|s| known(s, BROWSER_TIMEZONES)))
}

impl BrowserLocale {
    pub fn is_system(&self) -> bool {
        self.language.is_none() && self.timezone.is_none()
    }
    pub fn from_input(language: &str, timezone: &str) -> crate::Result<Self> {
        let parse = |value: &str, choices: &[&str], error: &str| {
            if value.trim().is_empty() {
                Ok(None)
            } else {
                known(value, choices)
                    .map(Some)
                    .ok_or_else(|| error.to_owned())
            }
        };
        Ok(Self {
            language: parse(
                language,
                BROWSER_LANGUAGES,
                "请选择说明中的浏览器语言，或留空跟随系统",
            )?,
            timezone: parse(
                timezone,
                BROWSER_TIMEZONES,
                "请选择说明中的时区，或留空跟随系统",
            )?,
        })
    }
    pub fn validate(&self) -> crate::Result<()> {
        Self::from_input(
            self.language.as_deref().unwrap_or_default(),
            self.timezone.as_deref().unwrap_or_default(),
        )
        .map(|_| ())
    }
}
