use dbus::arg::{RefArg, Variant};
use std::collections::HashMap;

/// Dbus library helper type
pub(crate) type VariantMap = HashMap<&'static str, Variant<Box<dyn RefArg>>>;
pub(crate) type VariantMapNested = HashMap<&'static str, HashMap<&'static str, Variant<Box<dyn RefArg>>>>;

pub fn add_val<V>(map: &mut VariantMap, key: &'static str, value: V)
where
    V: RefArg + 'static,
{
    map.insert(key, Variant(Box::new(value)));
}

pub fn add_str<V>(map: &mut VariantMap, key: &'static str, value: V)
where
    V: Into<String>,
{
    map.insert(key, Variant(Box::new(value.into())));
}
