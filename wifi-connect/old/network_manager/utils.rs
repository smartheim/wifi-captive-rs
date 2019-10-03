use crate::network_manager::errors::NetworkManagerError;
use crate::network_manager::VariantMap;
use ascii::AsciiStr;
use dbus::arg::{RefArg, Variant};

pub fn add_val<K, V>(map: &mut VariantMap, key: K, value: V)
where
    K: Into<String>,
    V: RefArg + 'static,
{
    map.insert(key.into(), Variant(Box::new(value)));
}

pub fn add_str<K, V>(map: &mut VariantMap, key: K, value: V)
where
    K: Into<String>,
    V: Into<String>,
{
    map.insert(key.into(), Variant(Box::new(value.into())));
}

pub fn add_string<K>(map: &mut VariantMap, key: K, value: String)
where
    K: Into<String>,
{
    map.insert(key.into(), Variant(Box::new(value)));
}

pub(crate) fn verify_ascii_password(password: String) -> Result<String, NetworkManagerError> {
    match AsciiStr::from_ascii(&password) {
        Err(_e) => Err(NetworkManagerError::pre_shared_key(
            "Not an ASCII password".into(),
        )),
        Ok(p) => {
            if p.len() < 8 {
                Err(NetworkManagerError::pre_shared_key(format!(
                    "Password length should be at least 8 characters: {} len",
                    p.len()
                )))
            } else if p.len() > 32 {
                Err(NetworkManagerError::pre_shared_key(format!(
                    "Password length should not exceed 64: {} len",
                    p.len()
                )))
            } else {
                Ok(password)
            }
        },
    }
}
