//! Password storage. Saved passwords live only in the macOS keychain.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

const SERVICE: &str = "app.lsdev.databases";

pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn set(&self, id: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// macOS keychain, service `app.lsdev.databases`, account = connection id.
pub struct KeychainStore;

fn entry(id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, id).map_err(|e| format!("Keychain: {e}"))
}

impl SecretStore for KeychainStore {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        match entry(id)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Keychain: {e}")),
        }
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        entry(id)?
            .set_password(secret)
            .map_err(|e| format!("Keychain: {e}"))
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        match entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Keychain: {e}")),
        }
    }
}

/// In-memory store, for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().map_err(|e| e.to_string())?.get(id).cloned())
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::default();
        assert_eq!(s.get("a"), Ok(None));
        s.set("a", "pw").unwrap();
        assert_eq!(s.get("a"), Ok(Some("pw".to_string())));
        s.delete("a").unwrap();
        assert_eq!(s.get("a"), Ok(None));
    }

    #[test]
    fn memory_store_delete_unknown_is_ok() {
        assert_eq!(MemoryStore::default().delete("nope"), Ok(()));
    }
}
