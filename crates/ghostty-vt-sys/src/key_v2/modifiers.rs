use super::InvalidKeyV2;

/// The ten defined public GhosttyMods bits. Side bits do not assert that
/// their corresponding base modifier is down, particularly on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModifiersV2(u16);
impl ModifiersV2 {
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
    pub fn try_from_bits(bits: u32) -> Result<Self, InvalidKeyV2> {
        if bits & !0x03ff != 0 {
            return Err(InvalidKeyV2::ModifierBits(bits));
        }
        u16::try_from(bits)
            .map(Self)
            .map_err(|_| InvalidKeyV2::ModifierBits(bits))
    }
    pub const fn bits(self) -> u16 {
        self.0
    }
    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
}
impl TryFrom<u32> for ModifiersV2 {
    type Error = InvalidKeyV2;
    fn try_from(bits: u32) -> Result<Self, Self::Error> {
        Self::try_from_bits(bits)
    }
}
impl std::ops::BitOr for ModifiersV2 {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
