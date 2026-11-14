//! Secret-bearing wrappers.
//!
//! * [`SecretText`] holds document plaintext (titles, bodies, tags, folder
//!   names). Its buffer is zeroized on drop and when it grows through
//!   [`SecretText::insert_str`], it never implements `Display`, its `Debug`
//!   output is redacted, and it does not implement `serde` traits: the only
//!   serialization path is the deliberate one in [`crate::format`].
//! * [`Passphrase`] wraps age's re-exported `secrecy::SecretString` so the type
//!   handed to the crypto library is the exact one it expects. It is never
//!   trimmed or normalized.
//!
//! Zeroizing one buffer does not clear every copy the process or the OS may
//! have made (allocator reuse, GUI layout caches, swap, clipboard managers).
//! See `docs/THREAT_MODEL.md` for the full list of residual copies.
//!
//! Secret types cannot be passed to the diagnostics API:
//!
//! ```compile_fail
//! use vault_core::{SecretText, diagnostics::{Diagnostics, Event}};
//! let mut d = Diagnostics::default();
//! d.record(SecretText::from("canary"));
//! ```
//!
//! and they cannot be formatted for display:
//!
//! ```compile_fail
//! let s = vault_core::SecretText::from("canary");
//! let _ = format!("{s}");
//! ```

use std::fmt;

use age::secrecy::{ExposeSecret, SecretString};
use zeroize::{Zeroize, Zeroizing};

/// Zeroizing plaintext string for document content and metadata.
#[derive(Default, PartialEq, Eq)]
pub struct SecretText {
    inner: String,
}

impl SecretText {
    /// Creates an empty secret string.
    pub const fn new() -> Self {
        Self {
            inner: String::new(),
        }
    }

    /// Creates an empty secret string with capacity reserved up front, so
    /// that later growth does not leave stale copies behind.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            inner: String::with_capacity(cap),
        }
    }

    /// Takes ownership of an existing `String` without copying it.
    pub fn from_string(s: String) -> Self {
        Self { inner: s }
    }

    /// Borrows the plaintext. Callers must not persist or log the result.
    pub fn expose(&self) -> &str {
        &self.inner
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Number of Unicode scalar values.
    pub fn char_count(&self) -> usize {
        self.inner.chars().count()
    }

    /// Replaces the content, zeroizing the previous buffer if a reallocation
    /// would otherwise be required.
    pub fn set(&mut self, value: &str) {
        if value.len() > self.inner.capacity() {
            let mut fresh =
                String::with_capacity(grown_capacity(self.inner.capacity(), value.len()));
            fresh.push_str(value);
            let mut old = std::mem::replace(&mut self.inner, fresh);
            old.zeroize();
        } else {
            // Clear in place (overwrite then truncate) and reuse the buffer.
            self.inner.zeroize();
            self.inner.push_str(value);
        }
    }

    /// Inserts `text` at byte offset `at` (which must be a char boundary).
    /// Growth zeroizes the old allocation instead of letting `String`
    /// reallocate and abandon a plaintext copy.
    pub fn insert_str(&mut self, at: usize, text: &str) {
        self.reserve_zeroizing(text.len());
        self.inner.insert_str(at, text);
    }

    /// Removes the byte range `start..end` (char boundaries).
    pub fn remove_range(&mut self, start: usize, end: usize) {
        // `drain` shifts the tail left; overwrite the now-unused tail bytes.
        let old_len = self.inner.len();
        self.inner.drain(start..end);
        let new_len = self.inner.len();
        wipe_spare(&mut self.inner, new_len, old_len);
    }

    /// Ensures capacity for `additional` more bytes, moving the content into
    /// a fresh allocation and zeroizing the old one if growth is required.
    pub fn reserve_zeroizing(&mut self, additional: usize) {
        let needed = self.inner.len().saturating_add(additional);
        if needed > self.inner.capacity() {
            let mut fresh = String::with_capacity(grown_capacity(self.inner.capacity(), needed));
            fresh.push_str(&self.inner);
            let mut old = std::mem::replace(&mut self.inner, fresh);
            old.zeroize();
        }
    }

    /// Clones into a new zeroizing buffer. Cloning is explicit so that copies
    /// are visible in review.
    pub fn duplicate(&self) -> Self {
        let mut s = String::with_capacity(self.inner.len());
        s.push_str(&self.inner);
        Self { inner: s }
    }

    /// Mutable access for editor widgets that require `&mut String`.
    ///
    /// Growth through the returned reference uses `String`'s normal
    /// reallocation and may leave a stale copy in freed memory; prefer
    /// [`Self::insert_str`]/[`Self::remove_range`] where possible.
    pub fn as_mut_string(&mut self) -> &mut String {
        &mut self.inner
    }

    /// Consumes the wrapper, returning a zeroizing `String`.
    pub fn into_zeroizing(mut self) -> Zeroizing<String> {
        Zeroizing::new(std::mem::take(&mut self.inner))
    }
}

fn grown_capacity(current: usize, needed: usize) -> usize {
    needed.max(current.saturating_mul(2)).max(64)
}

/// Overwrites bytes in `s`'s spare capacity between `from` and `to`.
fn wipe_spare(s: &mut String, from: usize, to: usize) {
    // Safe approach: temporarily extend with zero bytes then truncate.
    // NUL bytes are valid UTF-8, so the String invariant holds throughout.
    if to > from {
        let n = to - from;
        s.extend(std::iter::repeat_n('\0', n));
        s.truncate(from);
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        self.inner.zeroize();
    }
}

impl Zeroize for SecretText {
    fn zeroize(&mut self) {
        self.inner.zeroize();
    }
}

impl From<&str> for SecretText {
    fn from(s: &str) -> Self {
        let mut t = SecretText::with_capacity(s.len());
        t.inner.push_str(s);
        t
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretText(<redacted {} bytes>)", self.inner.len())
    }
}

/// A user passphrase. Never trimmed, normalized or logged.
pub struct Passphrase {
    inner: SecretString,
}

impl Passphrase {
    /// Wraps a passphrase exactly as entered.
    pub fn new(s: String) -> Self {
        Self {
            inner: SecretString::from(s),
        }
    }

    /// Whether the passphrase is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.expose_secret().is_empty()
    }

    /// Length in Unicode scalar values (for strength guidance only).
    pub fn char_count(&self) -> usize {
        self.inner.expose_secret().chars().count()
    }

    /// Compares two passphrases in time independent of where they differ.
    pub fn matches(&self, other: &Passphrase) -> bool {
        let a = self.inner.expose_secret().as_bytes();
        let b = other.inner.expose_secret().as_bytes();
        if a.len() != b.len() {
            return false;
        }
        a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }

    /// Explicit copy (e.g. to re-verify the current passphrase later).
    pub fn duplicate(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }

    /// Reveals the passphrase for an explicit user-requested display (the
    /// "show passphrase" control or a generated suggestion). Never log it.
    pub fn reveal(&self) -> &str {
        self.inner.expose_secret()
    }

    /// The secrecy type expected by the age crate. Crate-private: only the
    /// crypto adapter hands the passphrase to the library.
    pub(crate) fn to_age_secret(&self) -> SecretString {
        self.inner.clone()
    }

    /// Borrows the passphrase for local strength estimation.
    pub(crate) fn expose_for_strength(&self) -> &str {
        self.inner.expose_secret()
    }
}

impl From<&str> for Passphrase {
    fn from(s: &str) -> Self {
        Self::new(s.to_owned())
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let t = SecretText::from("CANARY-text-7781");
        let p = Passphrase::from("CANARY-pass-7781");
        let out = format!("{t:?} {p:?}");
        assert!(!out.contains("CANARY"));
        assert!(out.contains("redacted"));
    }

    #[test]
    fn passphrase_is_not_trimmed_or_normalized() {
        let a = Passphrase::from("  pass phrase \t");
        let b = Passphrase::from("pass phrase");
        assert!(!a.matches(&b));
        // NFC "é" vs NFD "e\u{301}" stay distinct.
        assert!(!Passphrase::from("caf\u{e9}").matches(&Passphrase::from("cafe\u{301}")));
        assert!(Passphrase::from("x y ").matches(&Passphrase::from("x y ")));
    }

    #[test]
    fn edits_preserve_content() {
        let mut t = SecretText::from("héllo");
        t.insert_str(t.len(), " wörld");
        assert_eq!(t.expose(), "héllo wörld");
        t.remove_range(0, "héllo ".len());
        assert_eq!(t.expose(), "wörld");
        t.set("a much longer replacement string than before");
        assert_eq!(t.expose(), "a much longer replacement string than before");
        t.set("short");
        assert_eq!(t.expose(), "short");
        let d = t.duplicate();
        assert_eq!(d.expose(), "short");
    }
}
