#![no_std]

extern crate alloc;

use alloc::{
    borrow::Cow,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedoxScheme<'a>(Cow<'a, str>);

impl<'a> RedoxScheme<'a> {
    pub fn new<S: Into<Cow<'a, str>>>(scheme: S) -> Option<Self> {
        let scheme = scheme.into();
        // Scheme cannot have NUL, /, or :
        if scheme.contains(&['\0', '/', ':']) {
            return None;
        }
        Some(Self(scheme))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedoxReference<'a>(Cow<'a, str>);

impl<'a> RedoxReference<'a> {
    pub fn new<S: Into<Cow<'a, str>>>(reference: S) -> Option<Self> {
        let reference = reference.into();
        // Reference cannot have NUL
        if reference.contains(&['\0']) {
            return None;
        }
        Some(Self(reference))
    }

    /// Join a [`RedoxReference`] with a path. Relative paths will be joined, absolute paths will
    /// be returned as-is.
    ///
    /// Returns `Some` on success and `None` if the path is not valid
    pub fn join<S: Into<Cow<'a, str>>>(&self, path: S) -> Option<Self> {
        let path = path.into();
        if path.starts_with('/') {
            // Absolute path, replaces reference
            Self::new(path)
        } else if path.is_empty() {
            // Empty path, return prior reference
            Self::new(self.0.clone())
        } else {
            // Relative path, append to reference
            let mut reference = self.0.clone().into_owned();
            if !reference.is_empty() && !reference.ends_with('/') {
                reference.push('/');
            }
            reference.push_str(&path);
            Self::new(reference)
        }
    }

    /// Canonicalize [`RedoxReference`], removing . and ..
    ///
    /// Returns `Some` on success and `None` if the path is not valid
    pub fn canonical(&self) -> Option<Self> {
        let canonical = {
            let parts = self
                .0
                .split('/')
                .rev()
                .scan(0, |nskip, part| {
                    if part == "." {
                        Some(None)
                    } else if part == ".." {
                        *nskip += 1;
                        Some(None)
                    } else if *nskip > 0 {
                        *nskip -= 1;
                        Some(None)
                    } else {
                        Some(Some(part))
                    }
                })
                .filter_map(|x| x)
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>();
            parts.iter().rev().fold(String::new(), |mut string, &part| {
                if !string.is_empty() && !string.ends_with('/') {
                    string.push('/');
                }
                string.push_str(part);
                string
            })
        };
        Self::new(canonical)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum RedoxPath<'a> {
    /// Standard UNIX compatible format
    Standard(RedoxReference<'a>),
    /// Legacy URI format
    Legacy(RedoxScheme<'a>, RedoxReference<'a>),
}

impl<'a> RedoxPath<'a> {
    /// Create [`RedoxPath`] from absolute path
    ///
    /// Returns `Some` on success and `None` if the path is not valid
    pub fn from_absolute(path: &'a str) -> Option<Self> {
        Some(if path.starts_with('/') {
            // New /scheme/scheme_name/path format
            Self::Standard(RedoxReference::new(path.trim_matches('/'))?)
        } else {
            // Old scheme_name:path format
            let mut parts = path.splitn(2, ':');
            let scheme = RedoxScheme::new(parts.next()?)?;
            let reference = RedoxReference::new(parts.next()?)?;
            Self::Legacy(scheme, reference)
        })
    }

    /// Join a [`RedoxPath`] with a path. Relative paths will be joined, absolute paths will be
    /// returned as-is.
    ///
    /// Returns `Some` on success and `None` if the path is not valid
    pub fn join(&self, path: &'a str) -> Option<Self> {
        if path.starts_with('/') {
            Self::from_absolute(path)
        } else {
            Some(match self {
                Self::Standard(reference) => Self::Standard(reference.join(path)?),
                Self::Legacy(scheme, reference) => {
                    Self::Legacy(scheme.clone(), reference.join(path)?)
                }
            })
        }
    }

    /// Canonicalize path, removing . and ..
    ///
    /// Returns `Some` on success and `None` if the path is not valid
    pub fn canonical(&self) -> Option<Self> {
        Some(match self {
            Self::Standard(reference) => Self::Standard(reference.canonical()?),
            Self::Legacy(scheme, reference) => {
                // We cannot canonicalize legacy paths since they may need to preserve dots and
                // slashes
                Self::Legacy(scheme.clone(), reference.clone())
            }
        })
    }
}

impl<'a> fmt::Display for RedoxPath<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedoxPath::Standard(reference) => {
                write!(f, "/{}", reference.0)
            }
            RedoxPath::Legacy(scheme, reference) => {
                write!(f, "{}:{}", scheme.0, reference.0)
            }
        }
    }
}

/// Make a relative path absolute using an optional current working directory.
///
/// Given a cwd of "scheme:/path", this his function will turn "foo" into "scheme:/path/foo".
/// "/foo" will turn into "file:/foo". "bar:/foo" will be used directly, as it is already
/// absolute
pub fn canonicalize_using_cwd<'a>(cwd_opt: Option<&str>, path: &'a str) -> Option<String> {
    let absolute = match RedoxPath::from_absolute(path) {
        Some(absolute) => absolute,
        None => {
            let cwd = cwd_opt?;
            let absolute = RedoxPath::from_absolute(cwd)?;
            absolute.join(path)?
        }
    };
    let canonical = absolute.canonical()?;
    Some(canonical.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    // Tests absolute paths without scheme
    #[test]
    fn test_absolute() {
        let cwd_opt = None;
        assert_eq!(canonicalize_using_cwd(cwd_opt, "/"), Some("/".to_string()));
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/file"),
            Some("/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/file"),
            Some("/folder/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/../file"),
            Some("/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/../.."),
            Some("/".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/../../../.."),
            Some("/".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/.."),
            Some("/".to_string())
        );
    }

    // Test relative paths using new scheme
    #[test]
    fn test_new_relative() {
        let cwd_opt = Some("/scheme/foo");
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "file"),
            Some("/scheme/foo/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/file"),
            Some("/scheme/foo/folder/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../file"),
            Some("/scheme/foo/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../.."),
            Some("/scheme".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../../../.."),
            Some("/".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, ".."),
            Some("/scheme".to_string())
        );
    }

    // Tests paths prefixed with /scheme/
    #[test]
    fn test_new_scheme() {
        let cwd_opt = None;
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/"),
            Some("/scheme/bar".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/file"),
            Some("/scheme/bar/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/file"),
            Some("/scheme/bar/folder/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/../file"),
            Some("/scheme/bar/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/../.."),
            Some("/scheme".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/../../../.."),
            Some("/".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/.."),
            Some("/scheme".to_string())
        );
    }

    // Test relative paths using old scheme
    #[test]
    fn test_old_relative() {
        let cwd_opt = Some("foo:");
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "file"),
            Some("foo:file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/file"),
            Some("foo:folder/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../file"),
            Some("foo:folder/../file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../.."),
            Some("foo:folder/../..".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../../../.."),
            Some("foo:folder/../../../..".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, ".."),
            Some("foo:..".to_string())
        );
    }

    // Tests paths prefixed with scheme_name:
    #[test]
    fn test_old_scheme() {
        let cwd_opt = None;
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:"),
            Some("bar:".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:file"),
            Some("bar:file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/file"),
            Some("bar:folder/file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/../file"),
            Some("bar:folder/../file".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/../.."),
            Some("bar:folder/../..".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/../../../.."),
            Some("bar:folder/../../../..".to_string())
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:.."),
            Some("bar:..".to_string())
        );
    }

    // Tests paths that may be used with orbital:
    #[test]
    fn test_orbital_scheme() {
        for flag_str in &["", "abflrtu"] {
            for x in &[-1, 0, 1] {
                for y in &[-1, 0, 1] {
                    for w in &[0, 1] {
                        for h in &[0, 1] {
                            for title in &[
                                "",
                                "title",
                                "title/with/slashes",
                                "title:with:colons",
                                "title/../with/../dots/..",
                            ] {
                                let path = format!(
                                    "orbital:{}/{}/{}/{}/{}/{}",
                                    flag_str, x, y, w, h, title
                                );
                                assert_eq!(canonicalize_using_cwd(None, &path), Some(path));
                            }
                        }
                    }
                }
            }
        }
    }
}
