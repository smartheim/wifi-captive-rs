use std::ascii;
use std::fmt;
use std::fmt::Write;
use std::str;
use std::convert::TryFrom;

use super::super::NetworkManagerError;

use serde::{Deserialize, Serialize};
use std::convert::From;
use std::string::FromUtf8Error;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ssid {
    vec: Vec<u8>,
}

impl Ssid {
    pub fn new() -> Self {
        Ssid { vec: Vec::new() }
    }

    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Ssid { vec: bytes.into() }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.vec
    }

    pub fn data(&self) -> &Vec<u8> {
        &self.vec
    }

    pub(crate) fn to_string(&self) -> Result<String, FromUtf8Error> {
        String::from_utf8(self.vec.clone())
    }
}

impl AsRef<[u8]> for Ssid {
    fn as_ref(&self) -> &[u8] {
        &self.vec
    }
}


impl TryFrom<String> for Ssid {
    type Error = NetworkManagerError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(&s as &str)
    }
}

impl<'a> TryFrom<&'a str> for Ssid {
    type Error = NetworkManagerError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.len() > 32 {
            Err(NetworkManagerError::ssid(format!(
                "ssid length should not exceed 32: {} len",
                s.len()
            )))
        } else {
            Ok(Ssid { vec: Vec::from(s) })
        }
    }
}

impl fmt::Debug for Ssid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_char('"')?;
        for byte in &self.vec {
            for c in ascii::escape_default(*byte) {
                f.write_char(c as char)?;
            }
        }
        f.write_char('"')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    #[test]
    fn test_ssid_from_bytes_as_bytes() {
        let vec_u8 = vec![0x68_u8, 0x65_u8, 0x6c_u8, 0x6c_u8, 0x6f_u8];
        let ssid = Ssid::from_bytes(vec_u8.clone());
        assert_eq!(vec_u8, ssid.vec);
    }

    #[test]
    fn test_ssid_from_bytes_eq() {
        let from_string: Ssid = "hello".try_into().unwrap();
        let vec_u8 = vec![0x68_u8, 0x65_u8, 0x6c_u8, 0x6c_u8, 0x6f_u8];
        let from_vec_u8 = Ssid::from_bytes(vec_u8);
        assert_eq!(from_string, from_vec_u8);
    }

    #[test]
    fn test_ssid_debug() {
        let ssid = Ssid::from_bytes(b"hello\0\x7F".to_vec());
        let debug = format!("{:?}", ssid);
        assert_eq!(debug, "\"hello\\x00\\x7f\"");
    }
}
