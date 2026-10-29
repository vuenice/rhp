//! Native Cryptographic Services for RHP (`Crypt::`).

pub mod aes;
pub mod hmac;
pub mod md5;

use std::fs::File;
use std::io::Read;

use base64::prelude::*;

pub use aes::{CipherMode, CipherSpec};
pub use hmac::HashAlgo;

/// Generate cryptographically secure random bytes from `/dev/urandom`.
pub fn random_bytes(length: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; length];
    if length == 0 {
        return Ok(buf);
    }

    // Try reading from /dev/urandom
    if let Ok(mut f) = File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            return Ok(buf);
        }
    }

    // Fallback: fast pseudo-random mix if file read fails (e.g. sandbox or restricted environment)
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0x517cc1b727220a95);
    for b in buf.iter_mut() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *b = (seed >> 33) as u8;
    }
    Ok(buf)
}

/// Compute hash digest of data using specified algorithm.
pub fn hash(algo_str: &str, data: &[u8], binary: bool) -> Result<Vec<u8>, String> {
    let algo = HashAlgo::parse(algo_str)
        .ok_or_else(|| format!("Unknown or unsupported hashing algorithm: '{}'", algo_str))?;
    let raw = algo.digest(data);
    if binary {
        Ok(raw)
    } else {
        Ok(hex::encode(raw).into_bytes())
    }
}

/// Compute HMAC using specified algorithm, key, and message data.
pub fn hash_hmac(algo_str: &str, data: &[u8], key: &[u8], binary: bool) -> Result<Vec<u8>, String> {
    let algo = HashAlgo::parse(algo_str)
        .ok_or_else(|| format!("Unknown or unsupported hashing algorithm: '{}'", algo_str))?;
    let raw = hmac::hmac(algo, key, data);
    if binary {
        Ok(raw)
    } else {
        Ok(hex::encode(raw).into_bytes())
    }
}

/// Encrypt data using OpenSSL-compatible AES cipher.
///
/// `options`: bitmask matching PHP constants:
/// - 1 (`OPENSSL_RAW_DATA`): input/output is raw binary instead of base64
/// - 2 (`OPENSSL_ZERO_PADDING`): do not apply PKCS#7 padding
pub fn openssl_encrypt(
    data: &[u8],
    cipher_algo: &str,
    passphrase: &[u8],
    options: i64,
    iv: &[u8],
) -> Result<Vec<u8>, String> {
    let spec = CipherSpec::parse(cipher_algo)
        .ok_or_else(|| format!("Unknown or unsupported cipher method: '{}'", cipher_algo))?;
    let raw_padding = (options & 2) != 0;
    let encrypted = aes::encrypt(spec, passphrase, iv, data, raw_padding)?;

    let raw_data = (options & 1) != 0;
    if raw_data {
        Ok(encrypted)
    } else {
        let b64 = BASE64_STANDARD.encode(&encrypted);
        Ok(b64.into_bytes())
    }
}

/// Decrypt data using OpenSSL-compatible AES cipher.
pub fn openssl_decrypt(
    data: &[u8],
    cipher_algo: &str,
    passphrase: &[u8],
    options: i64,
    iv: &[u8],
) -> Result<Vec<u8>, String> {
    let spec = CipherSpec::parse(cipher_algo)
        .ok_or_else(|| format!("Unknown or unsupported cipher method: '{}'", cipher_algo))?;
    let raw_data = (options & 1) != 0;
    let ciphertext = if raw_data {
        data.to_vec()
    } else {
        let text = std::str::from_utf8(data).map_err(|_| "Invalid base64 encoding".to_string())?;
        BASE64_STANDARD.decode(text.trim()).map_err(|e| format!("Base64 decode error: {}", e))?
    };

    let raw_padding = (options & 2) != 0;
    aes::decrypt(spec, passphrase, iv, &ciphertext, raw_padding)
}

/// List of supported hash algorithms.
pub fn hash_algos() -> &'static [&'static str] {
    &["md5", "sha1", "sha224", "sha256", "sha384", "sha512"]
}

/// List of supported OpenSSL cipher methods.
pub fn openssl_cipher_methods() -> &'static [&'static str] {
    &[
        "aes-128-cbc",
        "aes-128-ctr",
        "aes-128-ecb",
        "aes-192-cbc",
        "aes-192-ctr",
        "aes-192-ecb",
        "aes-256-cbc",
        "aes-256-ctr",
        "aes-256-ecb",
    ]
}
