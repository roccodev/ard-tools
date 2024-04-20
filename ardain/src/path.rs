use std::{fmt::Display, ops::Deref};

/// A valid (absolute) path in an ARH file system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArhPath(String);

impl ArhPath {
    pub fn join(&self, child: &str) -> Self {
        let mut new = self.clone();
        new.append(child);
        new
    }

    pub fn append(&mut self, child: &str) {
        if self.0.as_bytes().last() != Some(&b'/') {
            self.0.push('/');
        }
        if child.as_bytes().first() == Some(&b'/') {
            self.0.push_str(&child[1..]);
        } else {
            self.0.push_str(child);
        }
    }
}

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
