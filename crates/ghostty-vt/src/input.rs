/// Pointer modifiers. Keys use `key::KeyModifiers`, Ghostty's full bit set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}
impl Modifiers {
    pub(crate) fn bits(self) -> u32 {
        u32::from(self.shift)
            | u32::from(self.control) << 1
            | u32::from(self.alt) << 2
            | u32::from(self.super_key) << 3
    }
}
