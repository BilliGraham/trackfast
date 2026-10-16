//! Keeps the API token in the credential store of the operating system:
//! Windows Credential Manager, the macOS Keychain, or the Secret Service on Linux.

use super::ApiToken;

const SERVICE: &str = "trackfast";
const ACCOUNT: &str = "toggl-api-token";

pub type Error = keyring::Error;

fn entry() -> Result<keyring::Entry, Error> {
    keyring::Entry::new(SERVICE, ACCOUNT)
}

pub fn load() -> Result<Option<ApiToken>, Error> {
    match entry()?.get_password() {
        Ok(raw) => Ok(ApiToken::new(&raw)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn save(token: &ApiToken) -> Result<(), Error> {
    entry()?.set_password(token.expose())
}

pub fn delete() -> Result<(), Error> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error),
    }
}
