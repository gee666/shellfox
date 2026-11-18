//! Byte framing only. UTF-8 continuation bytes must not become C1 controls.
#[derive(Default)]
pub struct Decoder {
    pending: [u8; 4],
    len: usize,
    remaining: u8,
    historical: bool,
}

pub struct Token {
    bytes: [u8; 4],
    len: usize,
    pub control: Option<u8>,
    pub historical: bool,
}
impl Token {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Decoder {
    pub fn push(&mut self, byte: u8, historical: bool, mut emit: impl FnMut(Token)) {
        if self.remaining > 0 {
            if (0x80..=0xbf).contains(&byte) {
                self.pending[self.len] = byte;
                self.len += 1;
                self.remaining -= 1;
                if self.remaining == 0 {
                    let control = if self.len == 2 && self.pending[0] == 0xc2 {
                        Some(byte)
                    } else {
                        None
                    };
                    emit(self.token(control));
                }
                return;
            }
            // Keep malformed prefixes byte-exact, never decode lossily.
            emit(self.token(None));
            self.remaining = 0;
        }
        self.remaining = match byte {
            0xc2..=0xdf => 1,
            0xe0..=0xef => 2,
            0xf0..=0xf4 => 3,
            _ => 0,
        };
        self.pending = [byte, 0, 0, 0];
        self.len = 1;
        self.historical = historical;
        if self.remaining == 0 {
            emit(self.token(Some(byte)));
        }
    }
    fn token(&mut self, control: Option<u8>) -> Token {
        let token = Token {
            bytes: self.pending,
            len: self.len,
            control,
            historical: self.historical,
        };
        self.len = 0;
        token
    }
}
