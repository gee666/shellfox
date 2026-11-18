use crate::{validation, ProfileId, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub font_family: String,
    pub font_size: f32,
    pub accent_color: String,
    pub background_color: String,
    pub default_profile_id: ProfileId,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            font_family: "monospace".into(),
            font_size: 14.0,
            accent_color: "#ec4899".into(),
            background_color: "#111016".into(),
            default_profile_id: ProfileId::default(),
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        validation::text(&self.font_family, 200)?;
        validation::ensure(
            self.font_size.is_finite() && (8.0..=40.0).contains(&self.font_size),
            "Font size must be between 8 and 40.",
        )?;
        validation::color(&self.accent_color)?;
        validation::color(&self.background_color)
    }
    pub fn patched(&self, patch: PreferencesPatch) -> Result<Self> {
        let next = Self {
            font_family: patch
                .font_family
                .unwrap_or_else(|| self.font_family.clone()),
            font_size: patch.font_size.unwrap_or(self.font_size),
            accent_color: patch
                .accent_color
                .unwrap_or_else(|| self.accent_color.clone()),
            background_color: patch
                .background_color
                .unwrap_or_else(|| self.background_color.clone()),
            default_profile_id: patch
                .default_profile_id
                .unwrap_or_else(|| self.default_profile_id.clone()),
        };
        next.validate()?;
        Ok(next)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreferencesPatch {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub accent_color: Option<String>,
    pub background_color: Option<String>,
    pub default_profile_id: Option<ProfileId>,
}
