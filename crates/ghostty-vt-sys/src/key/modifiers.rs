use super::InvalidKey;

/// The ten defined public GhosttyMods bits. Side bits do not assert that
/// their corresponding base modifier is down, particularly on release.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "u32", into = "u32")]
pub struct KeyModifiers(u16);
impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);
    pub const CAPS_LOCK: Self = Self(1 << 4);
    pub const NUM_LOCK: Self = Self(1 << 5);
    pub const SHIFT_SIDE: Self = Self(1 << 6);
    pub const CONTROL_SIDE: Self = Self(1 << 7);
    pub const ALT_SIDE: Self = Self(1 << 8);
    pub const SUPER_SIDE: Self = Self(1 << 9);

    /// Preserves every defined-bit pattern, including right-side-only masks.
    pub fn try_from_bits(bits: u32) -> Result<Self, InvalidKey> {
        if bits & !0x03ff != 0 {
            return Err(InvalidKey::ModifierBits(bits));
        }
        u16::try_from(bits)
            .map(Self)
            .map_err(|_| InvalidKey::ModifierBits(bits))
    }
    pub const fn bits(self) -> u16 {
        self.0
    }
    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// Adds or removes `flag`.
    pub fn set(&mut self, flag: Self, on: bool) {
        if on {
            self.0 |= flag.0;
        } else {
            self.0 &= !flag.0;
        }
    }
}
impl TryFrom<u32> for KeyModifiers {
    type Error = InvalidKey;
    fn try_from(bits: u32) -> Result<Self, Self::Error> {
        Self::try_from_bits(bits)
    }
}
impl From<KeyModifiers> for u32 {
    fn from(mods: KeyModifiers) -> Self {
        u32::from(mods.bits())
    }
}
impl std::ops::BitOr for KeyModifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
