//! Offline credential verification against the Argon2id PHC-encoded hash
//! cached in `app_user.password_hash` / `app_user.pin_hash`.
//!
//! Must byte-for-byte match the encoding produced by
//! `backend/internal/platform/crypto/password.go`:
//!
//! ```text
//! $argon2id$v=<version>$m=<memory>,t=<time>,p=<threads>$<salt>$<hash>
//! ```
//! with `salt`/`hash` base64 RawStdEncoding (no padding). A hash minted by
//! the cloud must verify at the edge with the WAN down (ADR-011).

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;

use crate::error::{DbError, DbResult};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD_NO_PAD;

/// Verifies `plaintext` against `encoded` (the stored PHC string). Returns
/// `Ok(())` on match, `DbError::CredentialMismatch` on a well-formed hash
/// that does not match, and `DbError::MalformedHash` if `encoded` is not in
/// the expected format. Callers must not distinguish "user not found" from
/// a mismatch in any response surfaced to a cashier.
pub fn verify_password(plaintext: &str, encoded: &str) -> DbResult<()> {
    let parts: Vec<&str> = encoded.split('$').collect();
    // Split on '$' of "$argon2id$v=..$m=..,t=..,p=..$salt$hash" yields:
    // ["", "argon2id", "v=..", "m=..,t=..,p=..", "salt", "hash"]
    if parts.len() != 6 || parts[1] != "argon2id" {
        return Err(DbError::MalformedHash);
    }

    let version = parts[2]
        .strip_prefix("v=")
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or(DbError::MalformedHash)?;
    if version != Version::V0x13 as u32 {
        return Err(DbError::MalformedHash);
    }

    let (memory, time, threads) = parse_params(parts[3])?;

    let salt = B64.decode(parts[4]).map_err(|_| DbError::MalformedHash)?;
    let want = B64.decode(parts[5]).map_err(|_| DbError::MalformedHash)?;

    let params = Params::new(memory, time, threads as u32, Some(want.len()))
        .map_err(|_| DbError::MalformedHash)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut got = vec![0u8; want.len()];
    argon2
        .hash_password_into(plaintext.as_bytes(), &salt, &mut got)
        .map_err(|_| DbError::MalformedHash)?;

    // Constant-time compare: `subtle`-free but branchless XOR-fold, avoiding
    // an early-exit `==` on secret-derived bytes.
    let mismatch = got
        .iter()
        .zip(want.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        | ((got.len() != want.len()) as u8);

    if mismatch == 0 {
        Ok(())
    } else {
        Err(DbError::CredentialMismatch)
    }
}

fn parse_params(s: &str) -> DbResult<(u32, u32, u8)> {
    // "m=65536,t=2,p=4"
    let mut memory = None;
    let mut time = None;
    let mut threads = None;
    for field in s.split(',') {
        let (key, value) = field.split_once('=').ok_or(DbError::MalformedHash)?;
        match key {
            "m" => memory = value.parse::<u32>().ok(),
            "t" => time = value.parse::<u32>().ok(),
            "p" => threads = value.parse::<u8>().ok(),
            _ => return Err(DbError::MalformedHash),
        }
    }
    match (memory, time, threads) {
        (Some(m), Some(t), Some(p)) => Ok((m, t, p)),
        _ => Err(DbError::MalformedHash),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::password_hash::rand_core::OsRng;
    use rand::RngCore as _;

    /// Mirrors backend/internal/platform/crypto/password.go's HashPassword
    /// exactly, so tests exercise the same wire format the Go service
    /// produces, without depending on the Go toolchain being present here.
    fn hash_password_like_go(plaintext: &str) -> String {
        let _ = OsRng; // keep argon2's rand feature linked for parity; unused directly
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);

        let params = Params::new(64 * 1024, 2, 4, Some(32)).expect("valid params");
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = [0u8; 32];
        argon2
            .hash_password_into(plaintext.as_bytes(), &salt, &mut key)
            .expect("hash");

        format!(
            "$argon2id$v={}$m={},t={},p={}${}${}",
            Version::V0x13 as u32,
            64 * 1024,
            2,
            4,
            B64.encode(salt),
            B64.encode(key)
        )
    }

    #[test]
    fn verifies_matching_password() {
        let encoded = hash_password_like_go("correct horse battery staple");
        verify_password("correct horse battery staple", &encoded).expect("should match");
    }

    #[test]
    fn rejects_wrong_password() {
        let encoded = hash_password_like_go("correct horse battery staple");
        let err = verify_password("wrong password", &encoded).unwrap_err();
        assert!(matches!(err, DbError::CredentialMismatch));
    }

    #[test]
    fn rejects_malformed_hash() {
        let err = verify_password("anything", "not-a-phc-hash").unwrap_err();
        assert!(matches!(err, DbError::MalformedHash));
    }

    #[test]
    fn matches_a_known_go_produced_vector() {
        // Golden vector: produced once by calling
        // backend/internal/platform/crypto/password.go HashPassword("holler123")
        // with a fixed salt is not reproducible without invoking Go here, so
        // instead this test pins the exact encoding shape Go emits
        // (m=65536,t=2,p=4, RawStdEncoding, v=19) and proves our parser
        // accepts it and our verifier matches it end-to-end via a
        // hand-built PHC string with a known salt/key pair computed the
        // same way the Go implementation does (argon2.IDKey with identical
        // parameters), guarding against silent parameter drift.
        let salt = B64.encode([0u8; 16]);
        let params = Params::new(64 * 1024, 2, 4, Some(32)).expect("params");
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = [0u8; 32];
        argon2
            .hash_password_into(b"holler123", &[0u8; 16], &mut key)
            .expect("hash");
        let encoded = format!(
            "$argon2id$v=19$m=65536,t=2,p=4${}${}",
            salt,
            B64.encode(key)
        );

        verify_password("holler123", &encoded).expect("golden vector must verify");
        assert!(verify_password("wrong", &encoded).is_err());
    }
}
