//! Secure storage for the speech provider's API key using the OS credential vault.
//!
//! On Windows this maps to the Windows Credential Manager via the `keyring` crate.
//! The key is never written to the config file or logs. A single key slot is shared by
//! whichever provider is selected (OpenAI or Deepgram).

use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "VoiceForge";
const ACCOUNT: &str = "stt-api-key";

fn entry() -> Result<Entry> {
    Entry::new(SERVICE, ACCOUNT).context("opening credential vault entry")
}

/// Stores (or overwrites) the Deepgram API key in the OS credential vault.
pub fn set_api_key(key: &str) -> Result<()> {
    entry()?
        .set_password(key)
        .context("saving API key to credential vault")
}

/// Retrieves the Deepgram API key, or `None` if none has been stored yet.
pub fn get_api_key() -> Result<Option<String>> {
    match entry()?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("reading API key from credential vault"),
    }
}

/// Removes the stored API key, if present.
pub fn delete_api_key() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("deleting API key from credential vault"),
    }
}

/// Returns `true` if an API key is currently stored.
pub fn has_api_key() -> bool {
    matches!(get_api_key(), Ok(Some(_)))
}
