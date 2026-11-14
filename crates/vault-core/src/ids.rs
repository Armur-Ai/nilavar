//! Random 128-bit identifiers, rendered as 32 lowercase hex characters.

use std::fmt;

use crate::error::{ErrorCode, VaultError, VaultResult};

/// Fills a buffer from the OS CSPRNG. Failure is unrecoverable for a vault
/// application (we must never fall back to predictable identifiers).
pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    if getrandom::fill(&mut buf).is_err() {
        // Without an OS RNG we cannot create safe identifiers or files.
        panic!("operating system random number generator unavailable");
    }
    buf
}

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; 16]);

        impl $name {
            /// Generates a fresh random identifier.
            pub fn random() -> Self {
                Self(random_bytes::<16>())
            }

            /// Parses the 32-character lowercase hex form.
            pub fn parse(s: &str) -> VaultResult<Self> {
                parse_hex16(s).map(Self)
            }

            /// The 32-character lowercase hex form.
            pub fn to_hex(&self) -> String {
                to_hex(&self.0)
            }

            /// Short 8-character prefix for compact display.
            pub fn short(&self) -> String {
                to_hex(&self.0[..4])
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.short())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }
    };
}

id_type!(
    /// Identifies a vault across saves (stable for the vault's lifetime).
    VaultId
);
id_type!(
    /// Identifies one saved state of a vault.
    RevisionId
);
id_type!(
    /// Identifies a document.
    DocId
);
id_type!(
    /// Identifies a folder.
    FolderId
);
id_type!(
    /// Identifies a single history entry of a document.
    HistoryId
);

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn parse_hex16(s: &str) -> VaultResult<[u8; 16]> {
    let bytes = s.as_bytes();
    if bytes.len() != 32 {
        return Err(VaultError::new(ErrorCode::InvalidId));
    }
    let mut out = [0u8; 16];
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        let hi = hex_val(pair[0])?;
        let lo = hex_val(pair[1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(c: u8) -> VaultResult<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(VaultError::new(ErrorCode::InvalidId)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_reject() {
        let id = DocId::random();
        assert_eq!(DocId::parse(&id.to_hex()).unwrap(), id);
        assert!(DocId::parse("ABCDEF00000000000000000000000000").is_err());
        assert!(DocId::parse("abc").is_err());
        assert!(DocId::parse("zz000000000000000000000000000000").is_err());
        assert_ne!(DocId::random(), DocId::random());
    }
}
