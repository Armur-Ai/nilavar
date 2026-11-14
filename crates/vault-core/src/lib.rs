//! Nilavar vault core.
//!
//! This crate owns everything that touches vault plaintext or ciphertext: the
//! document model, the age encryption adapter, payload validation, failure-safe
//! storage, history, search and the closed diagnostics API. It has no GUI
//! dependency; the desktop app and the recovery CLI are thin consumers.
//!
//! First-party code in this crate is `#![forbid(unsafe_code)]`. Dependencies
//! (notably the age crate and its primitives) may contain unsafe code; see
//! `docs/DEPENDENCIES.md`.

#![forbid(unsafe_code)]

pub mod app;
pub mod autosave;
pub mod clipboard;
pub mod crypto;
pub mod diagnostics;
pub mod error;
pub mod format;
pub mod history;
pub mod ids;
pub mod io_export;
pub mod lifecycle;
pub mod limits;
pub mod markdown;
pub mod model;
pub mod paths;
pub mod search;
pub mod secret;
pub mod session;
pub mod storage;
pub mod strength;
pub mod textops;
pub mod time;

pub use error::{ErrorCode, VaultError, VaultResult};
pub use secret::{Passphrase, SecretText};
