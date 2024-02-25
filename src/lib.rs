#![feature(str_split_remainder)]
#![no_std]

extern crate alloc;

use alloc::{
    borrow::Cow,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

/// The name of a scheme
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedoxScheme<'a>(Cow<'a, str>);

impl<'a> RedoxScheme<'a> {
    /// Create a new [`RedoxScheme`], ensuring there are no invalid characters
    pub fn new<S: Into<Cow<'a, str>>>(scheme: S) -> Option<Self> {
        let scheme = scheme.into();
        // Scheme cannot have NUL, /, or :
        if scheme.contains(&['\0', '/', ':']) {
            return None;
        }
        Some(Self(scheme))
    }
}

impl<'a> AsRef<str> for RedoxScheme<'a> {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl<'a> fmt::Display for RedoxScheme<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The part of a path that is sent to each scheme
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedoxReference<'a>(Cow<'a, str>);

impl<'a> RedoxReference<'a> {
    /// Create a new [`RedoxReference`], ensuring there are no invalid characters
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

    /// Verify that the reference is canonicalized
    pub fn is_canon(&self) -> bool {
        self.0.is_empty()
            || self
                .0
                .split('/')
                .all(|seg| seg != ".." && seg != "." && seg != "")
    }
}

impl<'a> AsRef<str> for RedoxReference<'a> {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl<'a> fmt::Display for RedoxReference<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A fully qualified Redox path
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
            Self::Standard(RedoxReference::new(&path[1..])?)
        } else {
            // Old scheme_name:path format
            let mut parts = path.splitn(2, ':');
            let scheme = RedoxScheme::new(parts.next()?)?;
            let reference = RedoxReference::new(parts.next()?)?;
            Self::Legacy(scheme, reference)
        })
    }

    /// Join a [`RedoxPath`] with a &str path. Relative paths will be appended to self,
    /// absolute paths will be returned as a RedoxPath, ignoring self.
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

    /// Verify that the path is canonicalized.
    ///
    /// Returns false if any segment is ".", ".." or "".
    /// A path that is empty is allowed and is interpreted as "/".
    /// Legacy paths are assumed to be canonical.
    pub fn is_canon(&self) -> bool {
        match self {
            Self::Standard(reference) => reference.is_canon(),
            Self::Legacy(_scheme, _reference) => true,
        }
    }

    /// Convert into a [`RedoxScheme`] and [`RedoxReference`].
    /// - Standard paths will parse `/scheme/scheme_name/reference`, and anything not starting
    ///   with `/scheme` will be parsed as being part of the `file` scheme
    /// - Legacy paths can be instantly converted
    pub fn as_parts(&'a self) -> Option<(RedoxScheme<'a>, RedoxReference<'a>)> {
        if !self.is_canon() {
            return None;
        }
        match self {
            Self::Standard(reference) => {
                //TODO: this does not use the RedoxScheme::new and RedoxReference::new functions
                let mut parts = reference.0.split('/');
                loop {
                    match parts.next() {
                        Some("") => {
                            // Ignore empty parts
                        }
                        Some("scheme") => match parts.next() {
                            Some(scheme_name) => {
                                // Path is in /scheme/scheme_name
                                let remainder = parts.remainder().unwrap_or("");
                                return Some((
                                    RedoxScheme(Cow::from(scheme_name)),
                                    RedoxReference(Cow::from(remainder)),
                                ));
                            }
                            None => {
                                // Path is the root scheme
                                return Some((
                                    RedoxScheme(Cow::from("")),
                                    RedoxReference(Cow::from("")),
                                ));
                            }
                        },
                        _ => {
                            // If path has no special processing, it is inside the file scheme
                            return Some((RedoxScheme(Cow::from("file")), reference.clone()));
                        }
                    }
                }
            }
            Self::Legacy(scheme, reference) => {
                // Legacy paths are already split
                Some((scheme.clone(), reference.clone()))
            }
        }
    }

    /// Is the scheme for this path the same as the given string?
    pub fn matches_scheme(&self, other: &str) -> bool {
        if let Some((scheme, _)) = self.as_parts() {
            scheme.0 == other
        } else {
            false
        }
    }

    /// Does the scheme match the given category, e.g. "disk-"
    pub fn is_scheme_category(&self, category: &str) -> bool {
        if let Some((scheme, _)) = self.as_parts() {
            let mut parts = scheme.0.splitn(2, '.');
            if let Some(cat) = parts.next() {
                cat == category && parts.next().is_some()
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Is this the default scheme, "/scheme/file"?
    pub fn is_default_scheme(&self) -> bool {
        self.matches_scheme("file")
    }

    /// Is this a Legacy format path?
    pub fn is_legacy(&self) -> bool {
        match self {
            RedoxPath::Legacy(_, _) => true,
            _ => false,
        }
    }

    /// Format a [`RedoxPath`] into a UNIX style path
    pub fn to_standard(&self) -> String {
        match self {
            RedoxPath::Standard(reference) => {
                format!("/{}", reference.0)
            }
            RedoxPath::Legacy(scheme, reference) => {
                format!("/scheme/{}/{}", scheme.0, reference.0)
            }
        }
    }

    /// Format a [`RedoxPath`] into a UNIX style path,
    /// ensuring it is canonicalized
    pub fn to_standard_canon(&self) -> Option<String> {
        Some(match self {
            RedoxPath::Standard(reference) => {
                format!("/{}", reference.canonical()?.0)
            }
            RedoxPath::Legacy(scheme, reference) => {
                canonicalize_using_scheme(scheme.as_ref(), reference.as_ref().trim_start_matches('/'))?
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
/// Given a cwd of "/scheme/scheme_name/dir_name", this function will turn
/// path "foo" into /scheme/scheme_name/dir_name/foo".
/// "/foo" will be left as is, because it is already absolute.
/// "." and empty segments "//" will be removed.
/// ".." will be resolved by backing up one directory, except at the root,
/// where ".." will be ignored and removed.
///
/// For old format schemes,
/// given a cwd of "scheme:/path", this function will turn "foo" into "scheme:/path/foo".
/// "/foo" will turn into "file:/foo". "bar:/foo" will be used directly, as it is already
/// absolute.
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

/// Canonicalize as in [`canonicalize_using_cwd`], but Legacy format paths
/// are canonicalized into UNIX format
pub fn canonicalize_to_standard<'a>(cwd_opt: Option<&str>, path: &'a str) -> Option<String> {
    let absolute = match RedoxPath::from_absolute(path) {
        Some(absolute) => absolute,
        None => {
            let cwd = cwd_opt?;
            let absolute = RedoxPath::from_absolute(cwd)?;
            absolute.join(path)?
        }
    };
    absolute.to_standard_canon()
}

/// Make a path that is relative to the root of a scheme into a full path,
/// following the rules of [`canonicalize_using_cwd`].
///
/// Returns the Some if a valid path can be constructed,
/// None if the scheme name is not valid or some other error occurs
pub fn canonicalize_using_scheme<'a>(scheme: &str, path: &'a str) -> Option<String> {
    canonicalize_using_cwd(Some(&scheme_path(scheme)?), path)
}

/// The path for a given scheme name
///
/// Returns Some if the scheme name is valid, None otherwise
pub fn scheme_path(name: &str) -> Option<String> {
    let _ = RedoxScheme::new(name)?;
    canonicalize_using_cwd(Some("/scheme"), name)
}

/// Make a scheme name (not a full path) for a device of a given category
///
/// Returns Some if the resulting scheme name is valid, None otherwise
pub fn make_scheme_name(category: &str, detail: &str) -> Option<String> {
    let name = format!("{}.{}", category, detail);
    let _ = RedoxScheme::new(&name)?;
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, string::ToString};

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

        assert_eq!(
            canonicalize_using_scheme("bar", ""),
            Some("/scheme/bar".to_string())
        );
        assert_eq!(
            canonicalize_using_scheme("bar", "foo"),
            Some("/scheme/bar/foo".to_string())
        );
        assert_eq!(
            canonicalize_using_scheme("bar", ".."),
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

    // Tests path splitting to parts
    #[test]
    fn test_parts() {
        for (path, scheme, reference) in &[
            ("/foo/bar/baz", "file", "foo/bar/baz"),
            ("/scheme/foo/bar/baz", "foo", "bar/baz"),
            ("/", "file", ""),
            ("/bar", "file", "bar"),
            ("/...", "file", "..."),
        ] {
            let redox_path = RedoxPath::from_absolute(path).unwrap();
            let parts = redox_path.as_parts();
            assert_eq!(
                (path, parts),
                (
                    path,
                    Some((
                        RedoxScheme::new(*scheme).unwrap(),
                        RedoxReference::new(*reference).unwrap()
                    ))
                )
            );
            let to_string = format!("/scheme/{scheme}");
            let joined_path = RedoxPath::from_absolute(&to_string)
                .unwrap()
                .join(reference)
                .unwrap();
            if path.starts_with("/scheme") {
                assert_eq!(path, &format!("{joined_path}"));
            } else {
                assert_eq!(path, &format!("/{reference}"));
            }
        }

        // fail if the path is not absolute
        assert_eq!(RedoxPath::from_absolute("not/absolute"), None);

        // fail if the scheme is not properly canonicalized
        for path in [
            "//double/slash",
            "/ending/in/slash/",
            "/contains/dot/.",
            "/contains/dotdot/..",
        ] {
            let redox_path = RedoxPath::from_absolute(path).unwrap();
            let parts = redox_path.as_parts();
            assert_eq!((path, parts), (path, None));
        }
    }

    #[test]
    fn test_old_scheme_parts() {
        for (path, scheme, reference) in &[
            ("foo:bar/baz", "foo", "bar/baz"),
            ("emptyref:", "emptyref", ""),
            (":emptyscheme", "", "emptyscheme"),
        ] {
            let redox_path = RedoxPath::from_absolute(path).unwrap();
            let parts = redox_path.as_parts();
            assert_eq!(
                (path, parts),
                (
                    path,
                    Some((
                        RedoxScheme::new(*scheme).unwrap(),
                        RedoxReference::new(*reference).unwrap()
                    ))
                )
            );
        }

        // slash is not allowed in scheme names
        assert_eq!(RedoxPath::from_absolute("scheme/withslash:path"), None);
        // empty path is not allowed for from_absolute
        assert_eq!(RedoxPath::from_absolute(""), None)
    }

    #[test]
    fn test_matches() {
        assert!(RedoxPath::from_absolute("/scheme/foo")
            .unwrap()
            .matches_scheme("foo"));
        assert!(RedoxPath::from_absolute("/scheme/foo/bar")
            .unwrap()
            .matches_scheme("foo"));
        assert!(!RedoxPath::from_absolute("/scheme/foo")
            .unwrap()
            .matches_scheme("bar"));
        assert!(RedoxPath::from_absolute("foo:")
            .unwrap()
            .matches_scheme("foo"));
        assert!(RedoxPath::from_absolute(
            &canonicalize_using_cwd(Some("/scheme/foo"), "bar").unwrap()
        )
        .unwrap()
        .matches_scheme("foo"));
        assert!(
            RedoxPath::from_absolute(&canonicalize_using_cwd(Some("/foo"), "bar").unwrap())
                .unwrap()
                .matches_scheme("file")
        );
        assert!(RedoxPath::from_absolute(
            &canonicalize_using_cwd(Some("/scheme"), "foo/bar").unwrap()
        )
        .unwrap()
        .matches_scheme("foo"));
        assert!(RedoxPath::from_absolute("foo:/bar")
            .unwrap()
            .matches_scheme("foo"));
        assert!(!RedoxPath::from_absolute("foo:/bar")
            .unwrap()
            .matches_scheme("bar"));
        assert!(RedoxPath::from_absolute("/scheme/file")
            .unwrap()
            .is_default_scheme());
        assert!(!RedoxPath::from_absolute("/scheme/foo")
            .unwrap()
            .is_default_scheme());
        assert!(RedoxPath::from_absolute("file:bar")
            .unwrap()
            .is_default_scheme());
        assert!(RedoxPath::from_absolute("file:")
            .unwrap()
            .is_default_scheme());
        assert!(!RedoxPath::from_absolute("foo:bar")
            .unwrap()
            .is_default_scheme());
        assert!(RedoxPath::from_absolute("foo:bar").unwrap().is_legacy());
        assert!(!RedoxPath::from_absolute("/foo/bar").unwrap().is_legacy());
    }

    #[test]
    fn test_to_standard() {
        assert_eq!(
            &RedoxPath::from_absolute("foo:bar").unwrap().to_standard(),
            "/scheme/foo/bar"
        );
        assert_eq!(
            &RedoxPath::from_absolute("file:bar").unwrap().to_standard(),
            "/scheme/file/bar"
        );
        assert_eq!(
            &RedoxPath::from_absolute("/scheme/foo/bar")
                .unwrap()
                .to_standard(),
            "/scheme/foo/bar"
        );
        assert_eq!(
            &RedoxPath::from_absolute("/foo/bar").unwrap().to_standard(),
            "/foo/bar"
        );
        assert_eq!(
            &RedoxPath::from_absolute("foo:bar/../bar2")
                .unwrap()
                .to_standard_canon()
                .unwrap(),
            "/scheme/foo/bar2"
        );
        assert_eq!(
            &RedoxPath::from_absolute("file:bar/./../bar2")
                .unwrap()
                .to_standard_canon()
                .unwrap(),
            "/scheme/file/bar2"
        );
        assert_eq!(
            &RedoxPath::from_absolute("/scheme/file/bar/./../../foo/bar")
                .unwrap()
                .to_standard_canon()
                .unwrap(),
            "/scheme/foo/bar"
        );
        assert_eq!(
            &RedoxPath::from_absolute("/foo/bar")
                .unwrap()
                .to_standard_canon()
                .unwrap(),
            "/foo/bar"
        );
        assert_eq!(
            &canonicalize_to_standard(None, "/scheme/foo/bar").unwrap(),
            "/scheme/foo/bar"
        );
        assert_eq!(
            &canonicalize_to_standard(None, "foo:bar").unwrap(),
            "/scheme/foo/bar"
        );
        assert_eq!(
            &canonicalize_to_standard(None, "foo:bar/../..").unwrap(),
            "/scheme"
        );
        assert_eq!(
            &canonicalize_to_standard(None, "/scheme/foo/bar/..").unwrap(),
            "/scheme/foo"
        );
        assert_eq!(
            &canonicalize_to_standard(None, "foo:bar/bar2/..").unwrap(),
            "/scheme/foo/bar"
        );
    }

    #[test]
    fn test_scheme_path() {
        assert_eq!(scheme_path("foo"), Some("/scheme/foo".to_string()));
        assert_eq!(scheme_path(""), Some("/scheme".to_string()));
        assert_eq!(scheme_path("/foo"), None);
        assert_eq!(scheme_path("foo/bar"), None);
        assert_eq!(scheme_path("foo:"), None);
    }

    #[test]
    fn test_category() {
        assert_eq!(make_scheme_name("foo", "bar"), Some("foo.bar".to_string()));
        assert_eq!(
            RedoxPath::from_absolute(
                &scheme_path(&make_scheme_name("foo", "bar").unwrap()).unwrap()
            )
            .unwrap(),
            RedoxPath::Standard(RedoxReference::new("scheme/foo.bar").unwrap())
        );
        assert_eq!(make_scheme_name("foo", "/bar"), None);
        assert_eq!(make_scheme_name("foo", ":bar"), None);
        assert!(RedoxPath::from_absolute(
            &scheme_path(&make_scheme_name("foo", "bar").unwrap()).unwrap()
        )
        .unwrap()
        .is_scheme_category("foo"));
        assert!(RedoxPath::from_absolute("/scheme/foo.bar/bar2")
            .unwrap()
            .is_scheme_category("foo"));
        assert!(!RedoxPath::from_absolute("/scheme/foo/bar")
            .unwrap()
            .is_scheme_category("foo"));
        assert!(!RedoxPath::from_absolute("/foo.bar/bar2")
            .unwrap()
            .is_scheme_category("foo"));
    }
}
