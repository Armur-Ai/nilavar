//! Application identity. Rename the product by editing this one module.

/// Human-facing product name.
pub const APP_NAME: &str = "Nilavar";

/// Lowercase identifier used for directories and binaries.
pub const APP_SLUG: &str = "nilavar";

/// Reverse-DNS bundle identifier used by platform packaging.
///
/// This is a placeholder namespace and does not imply ownership of any domain.
pub const BUNDLE_ID: &str = "io.github.nilavar.Nilavar";

/// File extension for vault files (without the leading dot).
pub const VAULT_EXTENSION: &str = "nvault.age";

/// Neutral default vault filename. The external name, size and timestamps of
/// a vault are observable to anyone who can see the file, so the default
/// deliberately avoids describing its contents.
pub const DEFAULT_VAULT_FILENAME: &str = "vault.nvault.age";

/// Application version (from Cargo).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Release channel label shown in the UI and docs.
pub const RELEASE_LABEL: &str = "unaudited alpha";
