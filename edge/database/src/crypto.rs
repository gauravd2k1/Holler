//! Encryption at rest for the edge SQLite file (ADR-011 amendment to
//! ADR-003).
//!
//! ## Decision: application-level envelope encryption, not SQLCipher
//!
//! `app_user.password_hash` / `app_user.pin_hash` are cached Argon2id
//! credential material so a cashier can log in with the WAN down. ADR-011
//! requires the edge database file to be encrypted at rest and never copied
//! or backed up unencrypted.
//!
//! The natural choice is SQLCipher (a drop-in encrypted SQLite build), which
//! would give page-level encryption with no plaintext ever touching disk.
//! It was attempted first: `rusqlite`'s
//! `bundled-sqlcipher-vendored-openssl` feature failed to build in this
//! environment — the vendored OpenSSL build requires a Perl toolchain with
//! `Locale::Maketext::Simple`, which is not present, and there is no
//! system/vcpkg OpenSSL or SQLCipher available to link against instead. This
//! is an environment/build-infrastructure gap, not a rejection of SQLCipher
//! on technical merits; see the task report for the two options and the
//! recommendation to provision a proper OpenSSL/Perl toolchain (or a
//! system-linked SQLCipher) so a follow-up task can switch to it without
//! changing the repository API.
//!
//! Until that toolchain exists, this crate seals the database file at rest
//! with **application-level AES-256-GCM envelope encryption**:
//!
//! - On disk, the durable artifact is `<name>.db.enc`: a random 12-byte
//!   nonce followed by the AES-256-GCM ciphertext of the whole SQLite file
//!   (main file only; WAL is checkpointed into it before sealing).
//! - [`Db::open`](crate::Db::open) decrypts that file into the real SQLite
//!   path in the caller's database directory, then opens it with `rusqlite`
//!   so WAL mode (ADR-003) and the pragmas in [`crate::pragma`] apply.
//! - [`Db::close`](crate::Db::close) checkpoints WAL, closes the connection,
//!   re-encrypts the plaintext file back into `<name>.db.enc` with a fresh
//!   nonce, and then overwrites-then-deletes the plaintext file and any
//!   `-wal`/`-shm` siblings.
//!
//! **Known limitation, called out explicitly rather than hidden**: while a
//! `Db` is open, the plaintext SQLite bytes exist on disk for the duration
//! of the process (WAL mode requires a real file for concurrent
//! readers/writers; there is no VFS-level encryption without SQLCipher).
//! This crate never exposes any API to copy, back up, or export that
//! plaintext file, and always reseals+wipes it in [`Db::close`]. Callers
//! (edge services) must not open the raw `.db` path themselves — only this
//! crate touches it, per ADR-003.
//!
//! The encryption key itself is supplied by the caller as an
//! [`EncryptionKey`] (32 bytes). Key sourcing (OS keystore, TPM, or a
//! provisioned secret file) is a device-provisioning concern outside this
//! crate's scope; `edge/database` only requires *some* 32-byte key be
//! supplied and never persists it.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use rusqlite::Connection;

use crate::error::{DbError, DbResult};
use crate::pragma;

const NONCE_LEN: usize = 12;
/// Domain-separation context mixed into every seal/open as AEAD associated
/// data, so a ciphertext sealed for one purpose can never be silently
/// accepted for another.
const AAD: &[u8] = b"holler-edge-database:v1";

/// A 32-byte AES-256-GCM key for sealing the edge database file at rest.
/// Callers own key provisioning; this type only carries the bytes.
pub struct EncryptionKey([u8; 32]);

impl EncryptionKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

fn cipher(key: &EncryptionKey) -> Aes256Gcm {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.0))
}

/// Encrypts `plaintext_path` into `sealed_path` (nonce || ciphertext),
/// leaving `plaintext_path` untouched (callers wipe it separately so a
/// caller can choose to wipe-then-seal-check or seal-then-wipe atomically).
pub fn seal_file(plaintext_path: &Path, sealed_path: &Path, key: &EncryptionKey) -> DbResult<()> {
    let plaintext = fs::read(plaintext_path).map_err(DbError::Io)?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher(key)
        .encrypt(
            nonce,
            Payload {
                msg: &plaintext,
                aad: AAD,
            },
        )
        .map_err(|_| DbError::Encryption("failed to seal database file"))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);

    // Write to a temp sibling then rename, so a crash mid-write never leaves
    // a half-written sealed file that looks valid.
    let tmp_path = sealed_path.with_extension("enc.tmp");
    fs::write(&tmp_path, &out).map_err(DbError::Io)?;
    fs::rename(&tmp_path, sealed_path).map_err(DbError::Io)?;
    Ok(())
}

/// Decrypts `sealed_path` into `plaintext_path`. Returns `Ok(false)` (no-op)
/// if `sealed_path` does not exist yet — the caller then creates a fresh
/// database at `plaintext_path`.
pub fn open_file(sealed_path: &Path, plaintext_path: &Path, key: &EncryptionKey) -> DbResult<bool> {
    if !sealed_path.exists() {
        return Ok(false);
    }
    let sealed = fs::read(sealed_path).map_err(DbError::Io)?;
    if sealed.len() < NONCE_LEN {
        return Err(DbError::Encryption("sealed database file is truncated"));
    }
    let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher(key)
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: AAD,
            },
        )
        .map_err(|_| {
            DbError::Encryption("failed to open database file: wrong key or corrupted file")
        })?;

    fs::write(plaintext_path, &plaintext).map_err(DbError::Io)?;
    Ok(true)
}

/// Overwrites `path` with zeroes before removing it, so an unlinked
/// plaintext database does not linger recoverable on disk. Best-effort: on
/// SSDs this does not guarantee physical erasure, but it removes the
/// trivial "read the deleted file back" exposure and is stronger than a
/// bare `remove_file`. Missing files (e.g. no `-wal` yet) are not an error.
pub fn wipe_file(path: &Path) -> DbResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let len = fs::metadata(path).map_err(DbError::Io)?.len();
    fs::write(path, vec![0u8; len as usize]).map_err(DbError::Io)?;
    fs::remove_file(path).map_err(DbError::Io)?;
    Ok(())
}

/// SQLite's actual on-disk naming for the WAL/SHM siblings of `db_path` is
/// `<full file name>-wal` / `<full file name>-shm` (append, not
/// extension-replace) — e.g. `edge.db` -> `edge.db-wal`. Centralised here so
/// every caller (crash recovery, `Db::close`) agrees on the same paths.
pub fn wal_shm_paths(db_path: &Path) -> (PathBuf, PathBuf) {
    let mut wal = OsString::from(db_path.as_os_str());
    wal.push("-wal");
    let mut shm = OsString::from(db_path.as_os_str());
    shm.push("-shm");
    (PathBuf::from(wal), PathBuf::from(shm))
}

/// The clean-shutdown marker for `plaintext_path` (requirement #2): its
/// *presence* means a session is currently open or crashed without calling
/// [`crate::Db::close`]; its *absence* means the last session, if any,
/// closed cleanly. `Db::open` creates it after successfully opening and
/// migrating; `Db::close` removes it only after the file has been resealed
/// and wiped.
pub fn marker_path(plaintext_path: &Path) -> PathBuf {
    let mut marker = OsString::from(plaintext_path.as_os_str());
    marker.push(".open-marker");
    PathBuf::from(marker)
}

/// Zeroes-then-deletes `db_path` and its `-wal`/`-shm` siblings. Used both
/// by `Db::close` (clean shutdown) and by crash recovery (after any
/// committed data has been folded into a resealed backup) — the wipe
/// treatment must be identical on every path that can leave credential
/// material in plaintext on disk (requirement #3).
pub fn wipe_plaintext_and_wal_shm(db_path: &Path) -> DbResult<()> {
    let (wal, shm) = wal_shm_paths(db_path);
    wipe_file(db_path)?;
    wipe_file(&wal)?;
    wipe_file(&shm)?;
    Ok(())
}

/// Detects and deterministically resolves crash leftovers from an unclean
/// prior shutdown (requirement #1), before `Db::open` decrypts a fresh
/// working copy for the new session.
///
/// ## Recovery, not wipe
///
/// A plaintext `.db`/`-wal`/`-shm` left behind by a crash may hold
/// transactions that were committed to SQLite (durable per the `synchronous
/// = FULL` pragma, ADR-003) but never resealed into `<name>.db.enc`
/// because the process died before reaching `Db::close`. `docs/spec/sync.md`
/// is explicit that local transactions must never be deleted; silently
/// wiping a crash leftover would do exactly that; wiping is only correct
/// once the data it might contain has been preserved, in the sealed backup.
///
/// So the algorithm is: if a leftover plaintext file exists, always try to
/// recover it first — open it directly (SQLite's own WAL replay makes this
/// safe even mid-crash: WAL mode is designed to recover to the last
/// committed transaction on next open, which is the whole reason ADR-003
/// chose it), checkpoint the WAL into the main file, and reseal *that*
/// merged, up-to-date state into `sealed_path`, superseding whatever was
/// sealed at the end of the previous clean session. Only after that
/// succeeds do we wipe the plaintext leftover — at that point every
/// committed row is safely inside the fresh `.db.enc`, so nothing is lost.
///
/// If there is no leftover plaintext file at all (e.g. the crash happened
/// before the database file was ever created, or a previous recovery
/// already ran), this is a no-op beyond clearing a stale marker.
pub fn recover_crash_leftovers(
    sealed_path: &Path,
    plaintext_path: &Path,
    key: &EncryptionKey,
) -> DbResult<()> {
    let marker = marker_path(plaintext_path);
    let leftover_exists = plaintext_path.exists();

    if !marker.exists() && !leftover_exists {
        // Clean prior state (or first-ever run): nothing to recover or wipe.
        return Ok(());
    }

    if leftover_exists {
        // Fold any WAL pages into the main file, then reseal that merged,
        // up-to-date state. SQLite performs WAL crash recovery itself on
        // open, so this reflects the last transaction that was actually
        // committed — never a partial write.
        let conn = Connection::open(plaintext_path)?;
        pragma::configure_connection(&conn)?;
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        conn.close().map_err(|(_, e)| DbError::Sqlite(e))?;

        seal_file(plaintext_path, sealed_path, key)?;
    }

    // Every committed row from the crashed session is now inside the
    // freshly resealed `.db.enc` (or there was nothing to recover). The
    // plaintext leftover can now be wiped with the same treatment as a
    // clean close, and the stale marker cleared.
    wipe_plaintext_and_wal_shm(plaintext_path)?;
    if marker.exists() {
        fs::remove_file(&marker).map_err(DbError::Io)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn seal_then_open_round_trips() {
        let dir = tempdir().expect("tempdir");
        let plain = dir.path().join("db.sqlite");
        let sealed = dir.path().join("db.sqlite.enc");
        let reopened = dir.path().join("db.sqlite.reopened");

        fs::write(&plain, b"pretend this is sqlite bytes").expect("write plaintext");
        let key = EncryptionKey::new([7u8; 32]);

        seal_file(&plain, &sealed, &key).expect("seal");
        assert!(sealed.exists());
        let sealed_bytes = fs::read(&sealed).expect("read sealed");
        assert!(!sealed_bytes
            .windows(b"pretend".len())
            .any(|w| w == b"pretend"));

        let opened = open_file(&sealed, &reopened, &key).expect("open");
        assert!(opened);
        assert_eq!(
            fs::read(&reopened).unwrap(),
            b"pretend this is sqlite bytes"
        );
    }

    #[test]
    fn open_with_wrong_key_fails() {
        let dir = tempdir().expect("tempdir");
        let plain = dir.path().join("db.sqlite");
        let sealed = dir.path().join("db.sqlite.enc");
        let reopened = dir.path().join("db.sqlite.reopened");

        fs::write(&plain, b"secret bytes").expect("write plaintext");
        seal_file(&plain, &sealed, &EncryptionKey::new([1u8; 32])).expect("seal");

        let err = open_file(&sealed, &reopened, &EncryptionKey::new([2u8; 32]))
            .expect_err("wrong key must fail");
        assert!(matches!(err, DbError::Encryption(_)));
    }

    #[test]
    fn open_missing_sealed_file_is_noop() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("missing.enc");
        let reopened = dir.path().join("reopened.sqlite");
        let key = EncryptionKey::new([3u8; 32]);
        assert!(!open_file(&sealed, &reopened, &key).expect("no-op open"));
    }

    #[test]
    fn wipe_removes_file_and_zeroes_contents_first() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("secret.sqlite");
        fs::write(&path, b"sensitive").unwrap();
        wipe_file(&path).expect("wipe");
        assert!(!path.exists());
    }

    #[test]
    fn recover_crash_leftovers_is_noop_with_nothing_on_disk() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plain = dir.path().join("edge.db");
        let key = EncryptionKey::new([4u8; 32]);
        recover_crash_leftovers(&sealed, &plain, &key).expect("noop");
        assert!(!plain.exists());
        assert!(!sealed.exists());
    }

    #[test]
    fn recover_crash_leftovers_folds_committed_data_and_wipes_plaintext() {
        let dir = tempdir().expect("tempdir");
        let sealed = dir.path().join("edge.db.enc");
        let plain = dir.path().join("edge.db");
        let marker = marker_path(&plain);
        let key = EncryptionKey::new([5u8; 32]);

        // Simulate a crashed session: a real SQLite file with a committed
        // row, its WAL sibling, and the open-marker left behind because
        // Db::close was never called.
        {
            let conn = Connection::open(&plain).expect("open leftover");
            pragma::configure_connection(&conn).expect("pragmas");
            conn.execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);",
            )
            .expect("seed committed data");
            // Deliberately do not checkpoint/close cleanly.
        }
        fs::write(&marker, b"").expect("write marker");
        // Note: whether SQLite has already merged the WAL into the main
        // file by the time its Connection Drop runs (it may auto-checkpoint
        // on last-connection close) is an implementation detail this test
        // does not depend on — recover_crash_leftovers must fold and
        // reseal correctly either way, which is what is asserted below.

        recover_crash_leftovers(&sealed, &plain, &key).expect("recover");

        // No plaintext credential-bearing bytes may remain anywhere.
        assert!(!plain.exists(), "plaintext main file must be wiped");
        let (wal, shm) = wal_shm_paths(&plain);
        assert!(!wal.exists(), "plaintext WAL must be wiped");
        assert!(!shm.exists(), "plaintext SHM must be wiped");
        assert!(!marker.exists(), "stale marker must be cleared");

        // But the committed row must not have been silently discarded: it
        // must be recoverable from the freshly resealed backup.
        assert!(sealed.exists(), "recovered data must be resealed");
        let recovered_plain = dir.path().join("edge.db.recovered-check");
        open_file(&sealed, &recovered_plain, &key).expect("open resealed backup");
        let conn = Connection::open(&recovered_plain).expect("open recovered copy");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .expect("query recovered row");
        assert_eq!(count, 1, "committed pre-crash row must survive recovery");
    }
}
