//! FIPS 197 AES implementation supporting AES-128, AES-192, AES-256
//! in CBC, CTR, and ECB modes with PKCS#7 padding.

// AES S-box
const S_BOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

// AES Inverse S-box
const INV_S_BOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

const RCON: [u32; 11] = [
    0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36,
];

#[inline]
fn rot_word(w: u32) -> u32 {
    w.rotate_left(8)
}

#[inline]
fn sub_word(w: u32) -> u32 {
    let b0 = S_BOX[(w >> 24) as u8 as usize] as u32;
    let b1 = S_BOX[((w >> 16) & 0xff) as u8 as usize] as u32;
    let b2 = S_BOX[((w >> 8) & 0xff) as u8 as usize] as u32;
    let b3 = S_BOX[(w & 0xff) as u8 as usize] as u32;
    (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
}

#[inline]
fn xtime(x: u8) -> u8 {
    if (x & 0x80) != 0 {
        (x << 1) ^ 0x1b
    } else {
        x << 1
    }
}

#[inline]
fn multiply(mut x: u8, mut y: u8) -> u8 {
    let mut result = 0;
    while y != 0 {
        if (y & 1) != 0 {
            result ^= x;
        }
        x = xtime(x);
        y >>= 1;
    }
    result
}

#[derive(Debug, Clone)]
pub struct Aes {
    nb: usize,
    nr: usize,
    round_keys: Vec<u32>,
}

impl Aes {
    pub fn new(key: &[u8]) -> Result<Self, String> {
        let (nk, nr) = match key.len() {
            16 => (4, 10),
            24 => (6, 12),
            32 => (8, 14),
            _ => return Err(format!("Invalid AES key size: {} bytes (expected 16, 24, or 32)", key.len())),
        };
        let nb = 4;
        let mut round_keys = vec![0u32; nb * (nr + 1)];

        for i in 0..nk {
            round_keys[i] = u32::from_be_bytes([
                key[4 * i],
                key[4 * i + 1],
                key[4 * i + 2],
                key[4 * i + 3],
            ]);
        }

        for i in nk..nb * (nr + 1) {
            let mut temp = round_keys[i - 1];
            if i % nk == 0 {
                temp = sub_word(rot_word(temp)) ^ (RCON[i / nk] << 24);
            } else if nk > 6 && i % nk == 4 {
                temp = sub_word(temp);
            }
            round_keys[i] = round_keys[i - nk] ^ temp;
        }

        Ok(Self { nb, nr, round_keys })
    }

    pub fn encrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut state = [[0u8; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                state[r][c] = block[r + 4 * c];
            }
        }

        self.add_round_key(&mut state, 0);

        for round in 1..self.nr {
            self.sub_bytes(&mut state);
            self.shift_rows(&mut state);
            self.mix_columns(&mut state);
            self.add_round_key(&mut state, round);
        }

        self.sub_bytes(&mut state);
        self.shift_rows(&mut state);
        self.add_round_key(&mut state, self.nr);

        let mut out = [0u8; 16];
        for r in 0..4 {
            for c in 0..4 {
                out[r + 4 * c] = state[r][c];
            }
        }
        out
    }

    pub fn decrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut state = [[0u8; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                state[r][c] = block[r + 4 * c];
            }
        }

        self.add_round_key(&mut state, self.nr);

        for round in (1..self.nr).rev() {
            self.inv_shift_rows(&mut state);
            self.inv_sub_bytes(&mut state);
            self.add_round_key(&mut state, round);
            self.inv_mix_columns(&mut state);
        }

        self.inv_shift_rows(&mut state);
        self.inv_sub_bytes(&mut state);
        self.add_round_key(&mut state, 0);

        let mut out = [0u8; 16];
        for r in 0..4 {
            for c in 0..4 {
                out[r + 4 * c] = state[r][c];
            }
        }
        out
    }

    fn add_round_key(&self, state: &mut [[u8; 4]; 4], round: usize) {
        for c in 0..4 {
            let word = self.round_keys[round * self.nb + c];
            let bytes = word.to_be_bytes();
            for r in 0..4 {
                state[r][c] ^= bytes[r];
            }
        }
    }

    fn sub_bytes(&self, state: &mut [[u8; 4]; 4]) {
        for r in 0..4 {
            for c in 0..4 {
                state[r][c] = S_BOX[state[r][c] as usize];
            }
        }
    }

    fn inv_sub_bytes(&self, state: &mut [[u8; 4]; 4]) {
        for r in 0..4 {
            for c in 0..4 {
                state[r][c] = INV_S_BOX[state[r][c] as usize];
            }
        }
    }

    fn shift_rows(&self, state: &mut [[u8; 4]; 4]) {
        // row 1: shift left by 1
        let temp = state[1][0];
        state[1][0] = state[1][1];
        state[1][1] = state[1][2];
        state[1][2] = state[1][3];
        state[1][3] = temp;

        // row 2: shift left by 2
        state[2].swap(0, 2);
        state[2].swap(1, 3);

        // row 3: shift left by 3 (or right by 1)
        let temp = state[3][3];
        state[3][3] = state[3][2];
        state[3][2] = state[3][1];
        state[3][1] = state[3][0];
        state[3][0] = temp;
    }

    fn inv_shift_rows(&self, state: &mut [[u8; 4]; 4]) {
        // row 1: shift right by 1
        let temp = state[1][3];
        state[1][3] = state[1][2];
        state[1][2] = state[1][1];
        state[1][1] = state[1][0];
        state[1][0] = temp;

        // row 2: shift right by 2
        state[2].swap(0, 2);
        state[2].swap(1, 3);

        // row 3: shift right by 3
        let temp = state[3][0];
        state[3][0] = state[3][1];
        state[3][1] = state[3][2];
        state[3][2] = state[3][3];
        state[3][3] = temp;
    }

    fn mix_columns(&self, state: &mut [[u8; 4]; 4]) {
        for c in 0..4 {
            let a0 = state[0][c];
            let a1 = state[1][c];
            let a2 = state[2][c];
            let a3 = state[3][c];

            state[0][c] = multiply(2, a0) ^ multiply(3, a1) ^ a2 ^ a3;
            state[1][c] = a0 ^ multiply(2, a1) ^ multiply(3, a2) ^ a3;
            state[2][c] = a0 ^ a1 ^ multiply(2, a2) ^ multiply(3, a3);
            state[3][c] = multiply(3, a0) ^ a1 ^ a2 ^ multiply(2, a3);
        }
    }

    fn inv_mix_columns(&self, state: &mut [[u8; 4]; 4]) {
        for c in 0..4 {
            let a0 = state[0][c];
            let a1 = state[1][c];
            let a2 = state[2][c];
            let a3 = state[3][c];

            state[0][c] = multiply(0x0e, a0) ^ multiply(0x0b, a1) ^ multiply(0x0d, a2) ^ multiply(0x09, a3);
            state[1][c] = multiply(0x09, a0) ^ multiply(0x0e, a1) ^ multiply(0x0b, a2) ^ multiply(0x0d, a3);
            state[2][c] = multiply(0x0d, a0) ^ multiply(0x09, a1) ^ multiply(0x0e, a2) ^ multiply(0x0b, a3);
            state[3][c] = multiply(0x0b, a0) ^ multiply(0x0d, a1) ^ multiply(0x09, a2) ^ multiply(0x0e, a3);
        }
    }
}

/// Supported cipher mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherMode {
    Cbc,
    Ctr,
    Ecb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CipherSpec {
    pub key_size: usize,
    pub mode: CipherMode,
}

impl CipherSpec {
    pub fn parse(cipher: &str) -> Option<Self> {
        let clean = cipher.to_ascii_lowercase().replace('_', "-");
        match clean.as_str() {
            "aes-128-cbc" => Some(Self { key_size: 16, mode: CipherMode::Cbc }),
            "aes-192-cbc" => Some(Self { key_size: 24, mode: CipherMode::Cbc }),
            "aes-256-cbc" => Some(Self { key_size: 32, mode: CipherMode::Cbc }),
            "aes-128-ctr" => Some(Self { key_size: 16, mode: CipherMode::Ctr }),
            "aes-192-ctr" => Some(Self { key_size: 24, mode: CipherMode::Ctr }),
            "aes-256-ctr" => Some(Self { key_size: 32, mode: CipherMode::Ctr }),
            "aes-128-ecb" => Some(Self { key_size: 16, mode: CipherMode::Ecb }),
            "aes-192-ecb" => Some(Self { key_size: 24, mode: CipherMode::Ecb }),
            "aes-256-ecb" => Some(Self { key_size: 32, mode: CipherMode::Ecb }),
            _ => None,
        }
    }
}

/// Applies PKCS#7 padding to a block size of 16.
pub fn pkcs7_pad(data: &[u8]) -> Vec<u8> {
    let pad_len = 16 - (data.len() % 16);
    let mut out = Vec::with_capacity(data.len() + pad_len);
    out.extend_from_slice(data);
    out.extend(std::iter::repeat(pad_len as u8).take(pad_len));
    out
}

/// Removes PKCS#7 padding. Returns Err if invalid padding.
pub fn pkcs7_unpad(data: &[u8]) -> Result<&[u8], &'static str> {
    if data.is_empty() || data.len() % 16 != 0 {
        return Err("Invalid padded data length");
    }
    let pad_len = *data.last().unwrap() as usize;
    if pad_len == 0 || pad_len > 16 || pad_len > data.len() {
        return Err("Invalid PKCS7 padding length");
    }
    let unpadded_len = data.len() - pad_len;
    for &byte in &data[unpadded_len..] {
        if byte as usize != pad_len {
            return Err("Corrupted PKCS7 padding bytes");
        }
    }
    Ok(&data[..unpadded_len])
}

/// Derives exact-length key from passphrase (padding with zeroes or truncating).
pub fn normalize_key(passphrase: &[u8], target_len: usize) -> Vec<u8> {
    let mut key = vec![0u8; target_len];
    let copy_len = passphrase.len().min(target_len);
    key[..copy_len].copy_from_slice(&passphrase[..copy_len]);
    key
}

/// Encrypt data using AES cipher spec.
pub fn encrypt(spec: CipherSpec, key: &[u8], iv: &[u8], plaintext: &[u8], raw_padding: bool) -> Result<Vec<u8>, String> {
    let normalized_key = normalize_key(key, spec.key_size);
    let cipher = Aes::new(&normalized_key)?;

    match spec.mode {
        CipherMode::Cbc => {
            if iv.len() < 16 {
                return Err("IV must be at least 16 bytes for CBC mode".to_string());
            }
            let data = if raw_padding { plaintext.to_vec() } else { pkcs7_pad(plaintext) };
            let mut out = Vec::with_capacity(data.len());
            let mut prev_block = [0u8; 16];
            prev_block.copy_from_slice(&iv[..16]);

            for chunk in data.chunks_exact(16) {
                let mut block = [0u8; 16];
                for i in 0..16 {
                    block[i] = chunk[i] ^ prev_block[i];
                }
                let enc = cipher.encrypt_block(&block);
                out.extend_from_slice(&enc);
                prev_block = enc;
            }
            Ok(out)
        }
        CipherMode::Ctr => {
            if iv.len() < 16 {
                return Err("IV must be at least 16 bytes for CTR mode".to_string());
            }
            let mut counter = [0u8; 16];
            counter.copy_from_slice(&iv[..16]);
            let mut out = Vec::with_capacity(plaintext.len());

            for chunk in plaintext.chunks(16) {
                let keystream = cipher.encrypt_block(&counter);
                for i in 0..chunk.len() {
                    out.push(chunk[i] ^ keystream[i]);
                }
                // Increment 128-bit big-endian counter
                for i in (0..16).rev() {
                    counter[i] = counter[i].wrapping_add(1);
                    if counter[i] != 0 {
                        break;
                    }
                }
            }
            Ok(out)
        }
        CipherMode::Ecb => {
            let data = if raw_padding { plaintext.to_vec() } else { pkcs7_pad(plaintext) };
            let mut out = Vec::with_capacity(data.len());
            for chunk in data.chunks_exact(16) {
                let mut block = [0u8; 16];
                block.copy_from_slice(chunk);
                let enc = cipher.encrypt_block(&block);
                out.extend_from_slice(&enc);
            }
            Ok(out)
        }
    }
}

/// Decrypt data using AES cipher spec.
pub fn decrypt(spec: CipherSpec, key: &[u8], iv: &[u8], ciphertext: &[u8], raw_padding: bool) -> Result<Vec<u8>, String> {
    let normalized_key = normalize_key(key, spec.key_size);
    let cipher = Aes::new(&normalized_key)?;

    match spec.mode {
        CipherMode::Cbc => {
            if iv.len() < 16 {
                return Err("IV must be at least 16 bytes for CBC mode".to_string());
            }
            if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
                return Err("Ciphertext length must be a multiple of 16 in CBC mode".to_string());
            }
            let mut out = Vec::with_capacity(ciphertext.len());
            let mut prev_block = [0u8; 16];
            prev_block.copy_from_slice(&iv[..16]);

            for chunk in ciphertext.chunks_exact(16) {
                let mut cur_chunk = [0u8; 16];
                cur_chunk.copy_from_slice(chunk);
                let dec = cipher.decrypt_block(&cur_chunk);
                for i in 0..16 {
                    out.push(dec[i] ^ prev_block[i]);
                }
                prev_block = cur_chunk;
            }
            if raw_padding {
                Ok(out)
            } else {
                pkcs7_unpad(&out).map(|s| s.to_vec()).map_err(|e| e.to_string())
            }
        }
        CipherMode::Ctr => {
            if iv.len() < 16 {
                return Err("IV must be at least 16 bytes for CTR mode".to_string());
            }
            let mut counter = [0u8; 16];
            counter.copy_from_slice(&iv[..16]);
            let mut out = Vec::with_capacity(ciphertext.len());

            for chunk in ciphertext.chunks(16) {
                let keystream = cipher.encrypt_block(&counter);
                for i in 0..chunk.len() {
                    out.push(chunk[i] ^ keystream[i]);
                }
                for i in (0..16).rev() {
                    counter[i] = counter[i].wrapping_add(1);
                    if counter[i] != 0 {
                        break;
                    }
                }
            }
            Ok(out)
        }
        CipherMode::Ecb => {
            if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
                return Err("Ciphertext length must be a multiple of 16 in ECB mode".to_string());
            }
            let mut out = Vec::with_capacity(ciphertext.len());
            for chunk in ciphertext.chunks_exact(16) {
                let mut cur_chunk = [0u8; 16];
                cur_chunk.copy_from_slice(chunk);
                let dec = cipher.decrypt_block(&cur_chunk);
                out.extend_from_slice(&dec);
            }
            if raw_padding {
                Ok(out)
            } else {
                pkcs7_unpad(&out).map(|s| s.to_vec()).map_err(|e| e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_cbc_round_trip() {
        let key = b"0123456789abcdef0123456789abcdef"; // 32 bytes (AES-256)
        let iv = b"fedcba9876543210"; // 16 bytes
        let plaintext = b"Hello, RHP crypto world! This is a secret message.";

        let spec = CipherSpec::parse("aes-256-cbc").unwrap();
        let enc = encrypt(spec, key, iv, plaintext, false).unwrap();
        let dec = decrypt(spec, key, iv, &enc, false).unwrap();

        assert_eq!(dec, plaintext);
    }

    #[test]
    fn test_aes_ctr_round_trip() {
        let key = b"secret-key-16-by"; // 16 bytes (AES-128)
        let iv = b"init-vector-16-b"; // 16 bytes
        let plaintext = b"Arbitrary length streaming message";

        let spec = CipherSpec::parse("aes-128-ctr").unwrap();
        let enc = encrypt(spec, key, iv, plaintext, false).unwrap();
        let dec = decrypt(spec, key, iv, &enc, false).unwrap();

        assert_eq!(dec, plaintext);
    }

    #[test]
    fn test_aes_ecb_round_trip() {
        let key = b"my-192-bit-aes-key-here!"; // 24 bytes (AES-192)
        let iv = b"";
        let plaintext = b"ECB message block";

        let spec = CipherSpec::parse("aes-192-ecb").unwrap();
        let enc = encrypt(spec, key, iv, plaintext, false).unwrap();
        let dec = decrypt(spec, key, iv, &enc, false).unwrap();

        assert_eq!(dec, plaintext);
    }
}
