use std::{fmt::Display, ops::Deref};

/// A valid (absolute) path in an ARH file system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArhPath(String);

impl Default for ArhPath {
    fn default() -> Self {
        Self("/".to_string())
    }
}

impl From<String> for ArhPath {
    fn from(value: String) -> Self {
        Self(if value.starts_with("/") {
            value
        } else {
            format!("/{value}")
        })
    }
}

impl Display for ArhPath {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for ArhPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.as_str()
    }
}
