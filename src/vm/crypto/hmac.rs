//! RFC 2104 HMAC implementation in pure Rust.

use sha1::Digest as Sha1Digest;
use sha2::Digest as Sha2Digest;

use super::md5::md5_digest;

/// Supported hash algorithms for HMAC and standard hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgo {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgo {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "md5" => Some(Self::Md5),
            "sha1" => Some(Self::Sha1),
            "sha224" => Some(Self::Sha224),
            "sha256" => Some(Self::Sha256),
            "sha384" => Some(Self::Sha384),
            "sha512" => Some(Self::Sha512),
            _ => None,
        }
    }

    /// Block size in bytes for HMAC padding.
    pub fn block_size(self) -> usize {
        match self {
            Self::Md5 | Self::Sha1 | Self::Sha224 | Self::Sha256 => 64,
            Self::Sha384 | Self::Sha512 => 128,
        }
    }

    /// Output digest size in bytes.
    pub fn digest_size(self) -> usize {
        match self {
            Self::Md5 => 16,
            Self::Sha1 => 20,
            Self::Sha224 => 28,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    /// Compute single digest of data.
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Md5 => md5_digest(data).to_vec(),
            Self::Sha1 => {
                let mut hasher = sha1::Sha1::new();
                Sha1Digest::update(&mut hasher, data);
                Sha1Digest::finalize(hasher).to_vec()
            }
            Self::Sha224 => {
                let mut hasher = sha2::Sha224::new();
                Sha2Digest::update(&mut hasher, data);
                Sha2Digest::finalize(hasher).to_vec()
            }
            Self::Sha256 => {
                let mut hasher = sha2::Sha256::new();
                Sha2Digest::update(&mut hasher, data);
                Sha2Digest::finalize(hasher).to_vec()
            }
            Self::Sha384 => {
                let mut hasher = sha2::Sha384::new();
                Sha2Digest::update(&mut hasher, data);
                Sha2Digest::finalize(hasher).to_vec()
            }
            Self::Sha512 => {
                let mut hasher = sha2::Sha512::new();
                Sha2Digest::update(&mut hasher, data);
                Sha2Digest::finalize(hasher).to_vec()
            }
        }
    }
}

/// Compute HMAC using RFC 2104.
pub fn hmac(algo: HashAlgo, key: &[u8], data: &[u8]) -> Vec<u8> {
    let block_size = algo.block_size();

    let key_bytes: Vec<u8> = if key.len() > block_size {
        algo.digest(key)
    } else {
        key.to_vec()
    };

    let mut k_pad = vec![0u8; block_size];
    k_pad[..key_bytes.len()].copy_from_slice(&key_bytes);

    let mut ipad = vec![0x36u8; block_size];
    let mut opad = vec![0x5cu8; block_size];
    for i in 0..block_size {
        ipad[i] ^= k_pad[i];
        opad[i] ^= k_pad[i];
    }

    // Inner hash: H(ipad || data)
    let mut inner_input = Vec::with_capacity(block_size + data.len());
    inner_input.extend_from_slice(&ipad);
    inner_input.extend_from_slice(data);
    let inner_hash = algo.digest(&inner_input);

    // Outer hash: H(opad || inner_hash)
    let mut outer_input = Vec::with_capacity(block_size + inner_hash.len());
    outer_input.extend_from_slice(&opad);
    outer_input.extend_from_slice(&inner_hash);
    algo.digest(&outer_input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha256() {
        let mac = hmac(
            HashAlgo::Sha256,
            b"key",
            b"The quick brown fox jumps over the lazy dog",
        );
        assert_eq!(
            hex::encode(mac),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn test_hmac_md5() {
        let mac = hmac(
            HashAlgo::Md5,
            b"key",
            b"The quick brown fox jumps over the lazy dog",
        );
        assert_eq!(hex::encode(mac), "80070713463e7749b90c2dc24911e275");
    }

    #[test]
    fn test_hmac_sha1() {
        let mac = hmac(
            HashAlgo::Sha1,
            b"key",
            b"The quick brown fox jumps over the lazy dog",
        );
        assert_eq!(
            hex::encode(mac),
            "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9"
        );
    }
}
