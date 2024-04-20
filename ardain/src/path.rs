use std::{borrow::Cow, fmt::Display, ops::Deref, str::FromStr};

use thiserror::Error;

/// The maximum length of an absolute path in an ARH file system.
///
/// Includes the leading slash.
pub const ARH_PATH_MAX_LEN: usize = 256;
pub const ARH_PATH_ROOT: ArhPath = ArhPath(Cow::Borrowed("/"));

/// A valid (absolute) path in an ARH file system.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArhPath(Cow<'static, str>);

#[derive(Debug, Error)]
#[error("invalid path {path}: {desc}")]
pub struct InvalidPathError {
    path: String,
    desc: PathErrorDesc,
}

#[derive(Debug, Error)]
pub enum PathErrorDesc {
    #[error("ARH paths must have a leading slash")]
    NoLeadingSlash,
    #[error("consecutive slashes are not allowedin ARH paths")]
    ConsecutiveSlashes,
    #[error("ARH paths can be up to {ARH_PATH_MAX_LEN} characters in length")]
    TooLong,
    #[error("illegal character for an ARH path: {0}")]
    IllegalCharacter(char),
}

impl ArhPath {
    /// Converts a string to a valid path.
    ///
    /// This performs the following changes:
    ///
    /// * A leading forward-slash is inserted, if not already present
    /// * Back-slashes (`\`) are replaced with forward-slashes (`/`)
    /// * Consecutive forward-slashes are removed
    /// * Uppercase characters are changed to lowercase
    ///
    /// An error is returned if:
    /// * The string contains illegal (non-ASCII) characters
    /// * The string is longer than the maximum size ([`ARH_PATH_MAX_LEN`])
    pub fn normalize(value: impl AsRef<str>) -> Result<Self, InvalidPathError> {
        let mut new = String::with_capacity(value.as_ref().len() + 1);
        if !value.as_ref().chars().next().is_some_and(|c| c == '/') {
            new.push('/');
        }
        let mut last = '\0';
        for mut ch in value.as_ref().chars() {
            ch = ch.to_ascii_lowercase();
            if ch == '\\' {
                ch = '/';
            }
            if ch == '/' && last == '/' {
                // No consecutive slashes
                continue;
            }
            new.push(ch);
            last = ch;
        }
        Self::from_str(&new)
    }

    pub fn join(&self, child: &str) -> Self {
        self.try_join(child).unwrap()
    }

    pub fn try_join(&self, child: &str) -> Result<Self, InvalidPathError> {
        let mut new_str = self.0.to_string();
        if new_str.as_bytes().last() != Some(&b'/') {
            new_str.push('/');
        }
        if child.as_bytes().first() == Some(&b'/') {
            new_str.push_str(&child[1..]);
        } else {
            new_str.push_str(child);
        }
        Self::normalize(&new_str)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    /// Checks whether a character is legal for an ARH path.
    ///
    /// Note that while uppercase characters aren't allowed, this function still returns `true`
    /// for them. Uppercase characters either return another error type, or they are transparently
    /// converted to lowercase.
    ///  
    /// cf. ml::DevFileArchiveNx::normalizeFileName
    fn is_character_legal(chr: char) -> bool {
        chr.is_ascii() && chr != '\\'
    }
}

impl Default for ArhPath {
    fn default() -> Self {
        ARH_PATH_ROOT
    }
}

impl FromStr for ArhPath {
    type Err = InvalidPathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut path = s.to_string();
        if path.len() > ARH_PATH_MAX_LEN {
            return Err(InvalidPathError {
                path,
                desc: PathErrorDesc::TooLong,
            });
        }
        if !path.chars().next().is_some_and(|c| c == '/') {
            return Err(InvalidPathError {
                path,
                desc: PathErrorDesc::NoLeadingSlash,
            });
        }
        if let Some(chr) = path.chars().find(|&c| !Self::is_character_legal(c)) {
            return Err(InvalidPathError {
                path,
                desc: PathErrorDesc::IllegalCharacter(chr),
            });
        }
        if path
            .as_bytes()
            .windows(2)
            .any(|w| w[0] == w[1] && w[0] == b'/')
        {
            return Err(InvalidPathError {
                path,
                desc: PathErrorDesc::ConsecutiveSlashes,
            });
        }
        // Still normalize uppercase characters
        path.make_ascii_lowercase();
        Ok(ArhPath(path.into()))
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
        self.as_str()
    }
}
