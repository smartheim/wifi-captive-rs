use dbus::arg::{Array, Get, Iter, RefArg, Variant};
use dbus::ffidisp::stdintf::OrgFreedesktopDBusProperties;
use dbus::ffidisp::Connection as DBusConnection;
use dbus::ffidisp::{BusType,ConnPath};
use dbus::{Message, Path};

use super::super::NetworkManagerError;

const DEFAULT_TIMEOUT: u64 = 15;
const RETRIES_ALLOWED: usize = 10;

pub struct DBusApi {
    connection: DBusConnection,
    method_timeout: u64,
    base: &'static str,
    method_retry_error_names: &'static [&'static str],
}

impl DBusApi {
    pub fn new(
        base: &'static str,
        method_retry_error_names: &'static [&'static str],
        method_timeout: Option<u64>,
    ) -> Self {
        let connection = DBusConnection::get_private(BusType::System).unwrap();

        let method_timeout = method_timeout.unwrap_or(DEFAULT_TIMEOUT);

        DBusApi {
            connection,
            method_timeout,
            base,
            method_retry_error_names,
        }
    }

    pub fn method_timeout(&self) -> u64 {
        self.method_timeout
    }

    pub fn call(
        &self,
        path: &str,
        interface: &str,
        method: &str,
    ) -> Result<Message, NetworkManagerError> {
        self.call_with_args(path, interface, method, &[])
    }

    pub fn call_with_args(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        args: &[&dyn RefArg],
    ) -> Result<Message, NetworkManagerError> {
        self.call_with_args_retry(path, interface, method, args)
            .map_err(|_e| {
                let message = format!("{}::{} method call failed on {}", interface, method, path);
                error!("{}", message);
                NetworkManagerError::dbus_api(message)
            })
    }

    fn call_with_args_retry(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        args: &[&dyn RefArg],
    ) -> Result<Message, NetworkManagerError> {
        let mut retries = 0;

        loop {
            if let Some(result) = self.create_and_send_message(path, interface, method, args) {
                return result;
            }

            retries += 1;

            if retries == RETRIES_ALLOWED {
                return Err(NetworkManagerError::dbus_api(format!(
                    "Method call failed after {} retries",
                    RETRIES_ALLOWED
                )));
            }

            debug!(
                "Retrying {}::{} method call: retry #{}",
                interface, method, retries,
            );

            std::thread::sleep(::std::time::Duration::from_secs(1));
        }
    }

    fn create_and_send_message(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        args: &[&dyn RefArg],
    ) -> Option<Result<Message, NetworkManagerError>> {
        match Message::new_method_call(self.base, path, interface, method) {
            Ok(mut message) => {
                if !args.is_empty() {
                    message = message.append_ref(args);
                }

                self.send_message_checked(message)
            },
            Err(details) => Some(Err(NetworkManagerError::dbus_api(details).into())),
        }
    }

    fn send_message_checked(
        &self,
        message: Message,
    ) -> Option<Result<Message, NetworkManagerError>> {
        match self
            .connection
            .send_with_reply_and_block(message, self.method_timeout as i32 * 1000)
        {
            Ok(response) => Some(Ok(response)),
            Err(e) => {
                {
                    let name = e.name();
                    for error_name in self.method_retry_error_names {
                        if name == Some(error_name) {
                            debug!("Should retry D-Bus method call: {}", error_name);

                            return None;
                        }
                    }
                }

                Some(Err(NetworkManagerError::from(e)))
            },
        }
    }

    pub fn property<T>(
        &self,
        path: &str,
        interface: &str,
        name: &str,
    ) -> Result<T, NetworkManagerError>
    where
        DBusApi: VariantTo<T>,
    {
        let property_error = |details: &str, err: bool| {
            let message = format!(
                "Get {}::{} property failed on {}: {}",
                interface, name, path, details
            );
            if err {
                error!("{}", message);
            } else {
                debug!("{}", message);
            }
            Err(NetworkManagerError::dbus_api(message))
        };

        let path = self.with_path(path);

        match path.get(interface, name) {
            Ok(variant) => match DBusApi::variant_to(&variant) {
                Some(data) => Ok(data),
                None => property_error("wrong property type", true),
            },
            Err(e) => match e.message() {
                Some(details) => property_error(details, false),
                None => property_error("no details", false),
            },
        }
    }

    pub fn extract<'a, T>(&self, response: &'a Message) -> Result<T, NetworkManagerError>
    where
        T: Get<'a>,
    {
        response
            .get1()
            .ok_or_else(|| NetworkManagerError::dbus_api("Wrong response type".into()).into())
    }

    pub fn extract_two<'a, T1, T2>(
        &self,
        response: &'a Message,
    ) -> Result<(T1, T2), NetworkManagerError>
    where
        T1: Get<'a>,
        T2: Get<'a>,
    {
        let (first, second) = response.get2();

        if let Some(first) = first {
            if let Some(second) = second {
                return Ok((first, second));
            }
        }

        return Err(NetworkManagerError::dbus_api("Wrong response type".into()));
    }

    fn with_path<'a, P: Into<Path<'a>>>(&'a self, path: P) -> ConnPath<&'a DBusConnection> {
        self.connection
            .with_path(self.base, path, self.method_timeout as i32 * 1000)
    }
}

pub trait VariantTo<T> {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<T>;
}

impl VariantTo<String> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<String> {
        value.0.as_str().and_then(|v| Some(v.to_string()))
    }
}

impl VariantTo<i64> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<i64> {
        value.0.as_i64()
    }
}

impl VariantTo<u32> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<u32> {
        value.0.as_i64().and_then(|v| Some(v as u32))
    }
}

impl VariantTo<bool> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<bool> {
        value.0.as_i64().and_then(|v| Some(v == 0))
    }
}

impl VariantTo<Vec<String>> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<Vec<String>> {
        let mut result = Vec::new();

        if let Some(list) = value.0.as_iter() {
            for element in list {
                if let Some(string) = element.as_str() {
                    result.push(string.to_string());
                } else {
                    return None;
                }
            }

            Some(result)
        } else {
            None
        }
    }
}

impl VariantTo<Vec<u8>> for DBusApi {
    fn variant_to(value: &Variant<Box<dyn RefArg>>) -> Option<Vec<u8>> {
        let mut result = Vec::new();

        if let Some(list) = value.0.as_iter() {
            for element in list {
                if let Some(value) = element.as_i64() {
                    result.push(value as u8);
                } else {
                    return None;
                }
            }

            Some(result)
        } else {
            None
        }
    }
}

pub fn extract<'a, T>(var: &mut Variant<Iter<'a>>) -> Result<T, NetworkManagerError>
where
    T: Get<'a>,
{
    var.0.get::<T>().ok_or_else(|| {
        NetworkManagerError::dbus_api(format!("Variant type does not match: {:?}", var)).into()
    })
}

pub fn variant_iter_to_vec_u8(var: &mut Variant<Iter>) -> Result<Vec<u8>, NetworkManagerError> {
    let array_option = &var.0.get::<Array<u8, _>>();

    if let Some(array) = *array_option {
        Ok(array.collect())
    } else {
        Err(NetworkManagerError::dbus_api(format!(
            "Variant not an array: {:?}",
            var
        )))
    }
}

pub fn path_to_string(path: &Path) -> Result<String, NetworkManagerError> {
    if let Ok(slice) = path.as_cstr().to_str() {
        Ok(slice.to_string())
    } else {
        Err(NetworkManagerError::dbus_api(format!(
            "Path not nm_dbus_generated UTF-8 string: {:?}",
            path
        )))
    }
}
