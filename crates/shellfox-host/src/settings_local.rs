//! Fonts are device-local compatibility preferences, not global settings.
use crate::Preferences;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalPreferences {
    font_family: String,
    font_size: f32,
}
pub(crate) fn serialize<S: serde::Serializer>(
    prefs: &Preferences,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    LocalPreferences {
        font_family: prefs.font_family.clone(),
        font_size: prefs.font_size,
    }
    .serialize(serializer)
}
pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Preferences, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Legacy(Preferences),
        Local(LocalPreferences),
    }
    Ok(match Saved::deserialize(deserializer)? {
        Saved::Legacy(prefs) => prefs,
        Saved::Local(local) => Preferences {
            font_family: local.font_family,
            font_size: local.font_size,
            ..Preferences::default()
        },
    })
}
