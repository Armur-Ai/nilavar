//! Adapter over the `age` crate's passphrase (scrypt) mode.
//!
//! Nothing here implements cryptography: encryption, key derivation and
//! authentication are all performed by age. This module only
//!
//! * chooses and bounds the scrypt work factor,
//! * bounds input and output sizes before allocating,
//! * always finalizes the encryption stream and checks every I/O result,
//! * decrypts into a pre-sized zeroizing staging buffer and reads the stream
//!   to its authenticated end before returning anything, and
//! * maps library errors to fixed [`ErrorCode`]s.
//!
//! Every call to [`encrypt`] makes age generate a fresh file key, scrypt
//! salt and payload nonce.

use std::io::{self, Read, Write};

use age::scrypt;
use zeroize::Zeroizing;

use crate::error::{ErrorCode, VaultError, VaultResult};
use crate::limits;
use crate::secret::Passphrase;

/// A validated scrypt work factor (log2 N).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkFactor(u8);

impl WorkFactor {
    /// The production default ([`limits::DEFAULT_SCRYPT_LOG_N`]).
    pub const DEFAULT: WorkFactor = WorkFactor(limits::DEFAULT_SCRYPT_LOG_N);

    /// A low work factor for tests and fuzzing. Files remain valid age files.
    pub const TESTING: WorkFactor = WorkFactor(limits::MIN_SCRYPT_LOG_N);

    /// Validates a work factor within `[MIN, MAX]`.
    pub fn new(log_n: u8) -> VaultResult<Self> {
        if (limits::MIN_SCRYPT_LOG_N..=limits::MAX_SCRYPT_LOG_N).contains(&log_n) {
            Ok(Self(log_n))
        } else {
            Err(VaultError::new(ErrorCode::ExcessiveWorkFactor))
        }
    }

    /// The log2 N value.
    pub fn log_n(self) -> u8 {
        self.0
    }
}

impl Default for WorkFactor {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Encrypts `plaintext` to `out` with the passphrase, finalizing the stream.
/// Returns the writer so the caller can flush/sync it.
pub fn encrypt_to<W: Write>(
    passphrase: &Passphrase,
    work: WorkFactor,
    plaintext: &[u8],
    out: W,
) -> VaultResult<W> {
    let mut recipient = scrypt::Recipient::new(passphrase.to_age_secret());
    recipient.set_work_factor(work.log_n());
    let encryptor = age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
        .map_err(|_| VaultError::new(ErrorCode::EncryptFailed))?;
    let mut writer = encryptor
        .wrap_output(out)
        .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
    writer
        .write_all(plaintext)
        .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))?;
    writer
        .finish()
        .map_err(|e| VaultError::io(ErrorCode::WriteFailed, &e))
}

/// Encrypts into a new in-memory ciphertext buffer.
pub fn encrypt(passphrase: &Passphrase, work: WorkFactor, plaintext: &[u8]) -> VaultResult<Vec<u8>> {
    // Ciphertext is plaintext + header (~200 B) + 16 B tag per 64 KiB chunk.
    let cap = plaintext.len() + plaintext.len() / 4096 + 512;
    encrypt_to(passphrase, work, plaintext, Vec::with_capacity(cap))
}

/// Header facts that can be learned without the passphrase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderInfo {
    /// Whether the file is passphrase-encrypted (a single scrypt stanza).
    pub is_passphrase: bool,
}

/// Parses the age header only.
pub fn inspect_header(ciphertext: &[u8]) -> VaultResult<HeaderInfo> {
    if ciphertext.len() > limits::MAX_ENCRYPTED_BYTES {
        return Err(VaultError::new(ErrorCode::EncryptedInputTooLarge));
    }
    let d = age::Decryptor::new_buffered(ciphertext).map_err(map_decrypt_error)?;
    Ok(HeaderInfo {
        is_passphrase: d.is_scrypt(),
    })
}

/// Decrypts a complete age file held in memory.
///
/// * rejects inputs over [`limits::MAX_ENCRYPTED_BYTES`] before parsing;
/// * rejects non-passphrase files and work factors above
///   [`limits::MAX_SCRYPT_LOG_N`] before running the KDF at full cost;
/// * reads at most `max_plaintext` bytes into a pre-sized zeroizing buffer and
///   consumes the stream to its authenticated final chunk, so truncation,
///   reordering and tampering are detected before anything is returned.
pub fn decrypt(
    passphrase: &Passphrase,
    ciphertext: &[u8],
    max_plaintext: usize,
) -> VaultResult<Zeroizing<Vec<u8>>> {
    if ciphertext.len() > limits::MAX_ENCRYPTED_BYTES {
        return Err(VaultError::new(ErrorCode::EncryptedInputTooLarge));
    }
    let decryptor = age::Decryptor::new_buffered(ciphertext).map_err(map_decrypt_error)?;
    if !decryptor.is_scrypt() {
        return Err(VaultError::new(ErrorCode::NotPassphraseEncrypted));
    }
    let mut identity = scrypt::Identity::new(passphrase.to_age_secret());
    identity.set_max_work_factor(limits::MAX_SCRYPT_LOG_N);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(map_decrypt_error)?;

    // Plaintext is never longer than the ciphertext, so this capacity means
    // the staging buffer never reallocates (no stray plaintext copies).
    let cap = ciphertext.len().min(max_plaintext.saturating_add(1));
    let mut staging = Zeroizing::new(Vec::with_capacity(cap));
    let mut limited = reader.take(max_plaintext as u64 + 1);
    let mut chunk = Zeroizing::new([0u8; 16 * 1024]);
    loop {
        let n = match limited.read(&mut chunk[..]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(map_stream_error(&e)),
        };
        if staging.len() + n > max_plaintext {
            return Err(VaultError::new(ErrorCode::PayloadTooLarge));
        }
        if staging.len() + n > staging.capacity() {
            // Should not happen given the capacity bound; fail closed.
            return Err(VaultError::new(ErrorCode::Internal));
        }
        staging.extend_from_slice(&chunk[..n]);
    }
    Ok(staging)
}

fn map_decrypt_error(e: age::DecryptError) -> VaultError {
    use age::DecryptError as E;
    match e {
        E::DecryptionFailed | E::KeyDecryptionFailed | E::NoMatchingKeys => {
            VaultError::new(ErrorCode::WrongPassphraseOrNotAVault)
        }
        E::ExcessiveWork { .. } => VaultError::new(ErrorCode::ExcessiveWorkFactor),
        E::InvalidHeader => VaultError::new(ErrorCode::InvalidHeader),
        E::InvalidMac => VaultError::new(ErrorCode::HeaderTampered),
        E::UnknownFormat => VaultError::new(ErrorCode::UnknownAgeFormat),
        E::Io(io) => {
            if io.kind() == io::ErrorKind::UnexpectedEof {
                VaultError::new(ErrorCode::InvalidHeader)
            } else {
                VaultError::io(ErrorCode::ReadFailed, &io)
            }
        }
        #[allow(unreachable_patterns)]
        _ => VaultError::new(ErrorCode::InvalidHeader),
    }
}

fn map_stream_error(e: &io::Error) -> VaultError {
    match e.kind() {
        io::ErrorKind::UnexpectedEof => VaultError::new(ErrorCode::Truncated),
        io::ErrorKind::InvalidData => VaultError::new(ErrorCode::BodyTampered),
        _ => VaultError::new(ErrorCode::BodyTampered),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(s: &str) -> Passphrase {
        Passphrase::from(s)
    }

    #[test]
    fn roundtrip_and_fresh_state() {
        let p = pw("correct horse");
        let a = encrypt(&p, WorkFactor::TESTING, b"hello").unwrap();
        let b = encrypt(&p, WorkFactor::TESTING, b"hello").unwrap();
        assert_ne!(a, b, "salt/nonce/file key must be fresh per encryption");
        assert_eq!(&*decrypt(&p, &a, 100).unwrap(), b"hello");
        assert!(inspect_header(&a).unwrap().is_passphrase);
    }

    #[test]
    fn wrong_passphrase() {
        let c = encrypt(&pw("right"), WorkFactor::TESTING, b"x").unwrap();
        assert_eq!(
            decrypt(&pw("wrong"), &c, 100).unwrap_err().code(),
            ErrorCode::WrongPassphraseOrNotAVault
        );
    }

    #[test]
    fn payload_bound_enforced() {
        let p = pw("p");
        let c = encrypt(&p, WorkFactor::TESTING, &[7u8; 1000]).unwrap();
        assert_eq!(decrypt(&p, &c, 999).unwrap_err().code(), ErrorCode::PayloadTooLarge);
        assert_eq!(decrypt(&p, &c, 1000).unwrap().len(), 1000);
    }

    #[test]
    fn excessive_work_factor_rejected() {
        assert!(WorkFactor::new(limits::MAX_SCRYPT_LOG_N + 1).is_err());
        assert!(WorkFactor::new(limits::MIN_SCRYPT_LOG_N - 1).is_err());
    }
}
