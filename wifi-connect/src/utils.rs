use super::errors::CaptivePortalError;
use ascii::AsciiStr;

pub(crate) fn verify_ascii_password(password: String) -> Result<String, CaptivePortalError> {
    match AsciiStr::from_ascii(&password) {
        Err(_e) => Err(CaptivePortalError::pre_shared_key(
            "Not an ASCII password".into(),
        )),
        Ok(p) => {
            if p.len() < 8 {
                Err(CaptivePortalError::pre_shared_key(format!(
                    "Password length should be at least 8 characters: {} len",
                    p.len()
                )))
            } else if p.len() > 32 {
                Err(CaptivePortalError::pre_shared_key(format!(
                    "Password length should not exceed 64: {} len",
                    p.len()
                )))
            } else {
                Ok(password)
            }
        },
    }
}
