#![no_std]

extern crate alloc;

use alloc::{borrow::Cow, format, string::ToString};
use core::{ffi::CStr, fmt};

/// The default scheme. Currently hardcoded.
const DEFAULT_SCHEME: &str = "file";

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

    /// Similar to [`canonicalize_using_scheme`]
    pub fn canonicalize_as_scheme<'b>(&self, path: RedoxReference<'b>) -> Cow<'b, str> {
        canonicalize_using_scheme_checked(self, Err(path))
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
    /// Create a new [`RedoxReference`], ensuring there are no invalid characters.
    /// An empty path is considered valid and equivalent to '/'.
    /// A path not starting with '/' will be considered not canonical.
    pub fn new<S: Into<Cow<'a, str>>>(reference: S) -> Option<Self> {
        let reference = reference.into();
        // Reference cannot have NUL
        if reference.contains(&['\0']) {
            return None;
        }
        Some(Self(reference))
    }

    /// Create a new [`RedoxReference`] from C string, ensuring it is a valid UTF-8
    pub fn new_from_c<S: Into<&'a CStr>>(reference: S) -> Option<Self> {
        let reference = reference.into();
        Some(Self(Cow::Borrowed(reference.to_str().ok()?)))
    }

    /// Join a [`RedoxReference`] with a path. Relative paths will be joined, absolute paths will
    /// be returned as-is.
    ///
    /// Returns `Some` on success and `None` if the path is not valid
    pub fn join<'b, S: Into<Cow<'b, str>>>(&self, path: S) -> Option<RedoxReference<'b>> {
        let path = path.into();
        Some(self.join_checked(RedoxReference::new(path)?))
    }

    /// Join, but checked from NUL
    fn join_checked<'b>(&self, path: RedoxReference<'b>) -> RedoxReference<'b> {
        if path.0.starts_with('/') {
            // Absolute path, referenced
            path
        } else if path.0.is_empty() {
            // Empty path, return prior reference cloned
            RedoxReference(self.0.to_string().into())
        } else {
            // Relative path, append to reference
            let mut reference = self.0.clone().into_owned();
            if !reference.is_empty() && !reference.ends_with('/') {
                reference.push('/');
            }
            reference.push_str(&path.0);
            RedoxReference(reference.into())
        }
    }

    /// Canonicalize [`RedoxReference`], removing `.` and `..`.
    /// For compability with legacy scheme, a relative path will be remain relative.
    pub fn canonical(self) -> Self {
        let canonical = {
            let mut canonalized = true;
            let mut is_absolute = false;
            let mut parts = [""; 4096]; // PATH_MAX
            let mut parts_len = 0;
            for (i, part) in self.0.split('/').enumerate() {
                if part.is_empty() {
                    if i == 0 {
                        is_absolute = true;
                    } else {
                        canonalized = false;
                    }
                } else if part == "." {
                    canonalized = false;
                } else if part == ".." {
                    if parts_len > 0 {
                        parts_len -= 1;
                    }
                    canonalized = false;
                } else {
                    if parts_len >= 4096 {
                        break;
                    }
                    parts[parts_len] = part;
                    parts_len += 1;
                }
            }

            if canonalized {
                return self;
            }
            let mut string = if is_absolute { "/" } else { "" }.to_string();
            for (i, part) in parts[..parts_len].iter().enumerate() {
                if i > 0 {
                    string.push('/');
                }
                string.push_str(part);
            }
            string
        };
        // Does not use ::new() as &self is checked from NUL
        Self(Cow::Owned(canonical))
    }

    /// Convert [`RedoxReference`] into relative path if it's not already.
    pub fn to_relative(self) -> Self {
        if !self.0.starts_with('/') {
            return self;
        }
        match self.0 {
            Cow::Borrowed(s) => Self(s[1..].into()),
            Cow::Owned(s) => Self(s[1..].to_string().into()),
        }
    }

    /// Verify that the reference is canonicalized
    pub fn is_canon(&self) -> bool {
        (self.0.is_empty() || self.0 == "/")
            || (self.0.starts_with('/')
                && self
                    .0
                    .split('/')
                    .skip(1)
                    .all(|seg| seg != ".." && seg != "." && seg != ""))
    }

    pub fn dirname<'b>(&self) -> RedoxReference<'b> {
        self.join_checked(RedoxReference("..".into())).canonical()
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
    pub fn from_absolute<S: Into<Cow<'a, str>>>(path: S) -> Option<Self> {
        Self::from_absolute_checked(path).and_then(|s| s.ok())
    }

    /// Create a new [`RedoxPath`] from C string, ensuring it is a valid UTF-8
    pub fn from_absolute_c<S: Into<&'a CStr>>(path: S) -> Option<Self> {
        let path = path.into();
        let r = if matches!(path.to_bytes_with_nul().first(), Some(b'/')) {
            // New /path or /scheme/scheme_name/path format, referenced
            Self::Standard(RedoxReference::new_from_c(path)?)
        } else {
            // Old scheme_name:path format, copied
            let s = path.to_str().ok()?;
            Self::from_absolute(s)?
        };
        Some(r)
    }

    /// Create [`RedoxPath`] from absolute path, or return relative path if valid
    fn from_absolute_checked<S: Into<Cow<'a, str>>>(path: S) -> Option<RedoxPathStr<'a>> {
        let path = path.into();
        let r = if path.starts_with('/') {
            // New /path or /scheme/scheme_name/path format, referenced
            Self::Standard(RedoxReference::new(path)?)
        } else {
            // Old scheme_name:path format, copied
            let mut parts = path.as_ref().splitn(2, ':');
            let (first, second) = match (parts.next(), parts.next()) {
                (Some(a), Some(b)) => (a, b),
                // Neither an old scheme, so it's relative
                _ => return Some(Err(RedoxReference::new(path)?)),
            };
            let scheme = RedoxScheme::new(first.to_string())?;
            let reference = RedoxReference::new(second.to_string())?;
            Self::Legacy(scheme, reference)
        };
        Some(Ok(r))
    }

    /// Join a [`RedoxPath`] with a &str path. Relative paths will be appended to self,
    /// absolute paths will be returned as a RedoxPath, ignoring self.
    ///
    /// Returns `Some` on success and `None` if the path is not valid
    pub fn join<'b>(&self, path: &'b str) -> Option<RedoxPath<'b>> {
        Some(self.join_checked(RedoxReference::new(path)?))
    }

    fn join_checked<'b>(&self, path: RedoxReference<'b>) -> RedoxPath<'b> {
        if path.0.starts_with('/') {
            // Absolute path, replaces reference
            RedoxPath::Standard(path)
        } else {
            match self {
                Self::Standard(reference) => RedoxPath::Standard(reference.join_checked(path)),
                Self::Legacy(scheme, reference) => RedoxPath::Legacy(
                    RedoxScheme(scheme.0.to_string().into()),
                    reference.join_checked(path),
                ),
            }
        }
    }

    /// Canonicalize path, removing `.` and `..`.
    pub fn canonical(self) -> Self {
        match self {
            Self::Standard(reference) => Self::Standard(reference.canonical()),
            Self::Legacy(scheme, reference) => {
                // We cannot canonicalize legacy paths since they may need to preserve dots and
                // slashes
                Self::Legacy(scheme, reference)
            }
        }
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
        let (scheme, path) = self.get_scheme_prefixed(true)?;
        Some((scheme, RedoxReference(path.into())))
    }

    /// Get a [`RedoxScheme`] if the reference contained `/scheme/scheme_name`.
    /// Does not fallback to default scheme if absent.
    pub fn get_scheme(&'a self) -> Option<RedoxScheme<'a>> {
        Some(self.get_scheme_prefixed(false)?.0)
    }

    fn get_scheme_prefixed(&'a self, fallback: bool) -> Option<(RedoxScheme<'a>, &'a str)> {
        const SCHEME_PREFIX_LENGTH: usize = "/scheme/".len();
        if !self.is_canon() {
            return None;
        }
        match self {
            Self::Standard(reference) => {
                let mut parts = reference.0.split('/');
                loop {
                    match parts.next() {
                        Some("") => {
                            // Ignore empty parts
                        }
                        Some("scheme") => match parts.next() {
                            Some(scheme_name) => {
                                // Path is in /scheme/scheme_name
                                let scheme_length = SCHEME_PREFIX_LENGTH + scheme_name.len() + 1;
                                let remainder = reference.0.get(scheme_length..).unwrap_or("");

                                return Some((RedoxScheme::new(scheme_name)?, remainder));
                            }
                            None => {
                                // Path is the root scheme
                                return Some((RedoxScheme(Cow::from("")), ""));
                            }
                        },
                        _ => {
                            if !fallback {
                                return None;
                            }
                            // If path has no special processing, it is inside the file scheme
                            let remainder = reference.0.get(1..).unwrap_or("");
                            return Some((RedoxScheme(Cow::from(DEFAULT_SCHEME)), remainder));
                        }
                    }
                }
            }
            Self::Legacy(scheme, reference) => {
                // Legacy paths are already split
                Some((scheme.clone(), reference.as_ref()))
            }
        }
    }

    /// Is the scheme for this path the same as the given string?
    pub fn matches_scheme(&self, other: &str) -> bool {
        if let Some((scheme, _)) = self.get_scheme_prefixed(true) {
            scheme.0 == other
        } else {
            false
        }
    }

    /// Does the scheme match the given category, e.g. "disk-"
    pub fn is_scheme_category(&self, category: &str) -> bool {
        if let Some((scheme, _)) = self.get_scheme_prefixed(true) {
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
        self.matches_scheme(DEFAULT_SCHEME)
    }

    /// Is this a Legacy format path?
    pub fn is_legacy(&self) -> bool {
        match self {
            RedoxPath::Legacy(_, _) => true,
            _ => false,
        }
    }

    /// Format a [`RedoxPath`] into a UNIX style path
    pub fn to_standard(self) -> Cow<'a, str> {
        self.to_standard_checked().0
    }

    fn to_standard_checked(self) -> RedoxReference<'a> {
        match self {
            RedoxPath::Standard(reference) => reference,
            RedoxPath::Legacy(scheme, reference) => {
                RedoxReference(format!("/scheme/{}/{}", scheme.0, reference.0).into())
            }
        }
    }

    /// Format a [`RedoxPath`] into a UNIX style path,
    /// ensuring it is canonicalized
    pub fn to_standard_canon(self) -> Cow<'a, str> {
        match self {
            RedoxPath::Standard(reference) => reference.canonical().0,
            RedoxPath::Legacy(scheme, reference) => {
                canonicalize_using_scheme_checked(&scheme, Ok(RedoxPath::Standard(reference)))
            }
        }
    }

    pub fn dirname<'b>(&self) -> RedoxPath<'b> {
        match self {
            RedoxPath::Standard(redox_reference) => RedoxPath::Standard(redox_reference.dirname()),
            RedoxPath::Legacy(redox_scheme, redox_reference) => RedoxPath::Legacy(
                RedoxScheme(redox_scheme.to_string().into()),
                redox_reference.dirname(),
            ),
        }
    }

    /// Similar to [`canonicalize_using_cwd`]
    pub fn canonicalize_as_cwd<'b>(&self, path: RedoxReference<'b>) -> Cow<'b, str> {
        // SAFETY: canonicalize_using_cwd_checked will None only if cwd_opt is None
        canonicalize_using_cwd_checked(Some(self), Err(path), false).unwrap()
    }

    /// Similar to [`canonicalize_to_standard`]
    pub fn canonicalize_as_cwd_standard<'b>(&self, path: RedoxReference<'b>) -> Cow<'b, str> {
        // SAFETY: canonicalize_to_standard_checked will None only if cwd_opt is None
        canonicalize_to_standard_checked(Some(self), Err(path)).unwrap()
    }
}

impl<'a> fmt::Display for RedoxPath<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedoxPath::Standard(reference) => {
                write!(f, "{}", reference.0)
            }
            RedoxPath::Legacy(scheme, reference) => {
                write!(f, "{}:{}", scheme.0, reference.0)
            }
        }
    }
}

impl<'a> From<RedoxPath<'a>> for Cow<'a, str> {
    fn from(value: RedoxPath<'a>) -> Self {
        match value {
            RedoxPath::Standard(redox_reference) => redox_reference.0,
            a => Cow::Owned(a.to_string()),
        }
    }
}

/// A valid path (checked against NUL) that can be absolute or relative
type RedoxPathStr<'a> = Result<RedoxPath<'a>, RedoxReference<'a>>;

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
pub fn canonicalize_using_cwd<'a>(cwd_opt: Option<&'a str>, path: &'a str) -> Option<Cow<'a, str>> {
    let cwd_opt = cwd_opt.and_then(|s| RedoxPath::from_absolute(Cow::Borrowed(s)));
    canonicalize_using_cwd_checked(
        cwd_opt.as_ref(),
        RedoxPath::from_absolute_checked(path)?,
        false,
    )
}

/// `canonicalize_using_cwd` but for internal use.
/// `in_scheme_root`: Treat cwd_opt as the root of scheme, similar to
///  openat RESOLVE_IN_ROOT but still allowing access to other scheme root.
fn canonicalize_using_cwd_checked<'a, 'b>(
    cwd_opt: Option<&RedoxPath<'a>>,
    path: RedoxPathStr<'b>,
    in_scheme_root: bool,
) -> Option<Cow<'b, str>> {
    let absolute = match (path, in_scheme_root) {
        (Ok(absolute), true) if absolute.get_scheme().is_some() => absolute,
        (Ok(absolute), true) => cwd_opt?.join_checked(absolute.to_standard_checked().to_relative()),
        (Ok(absolute), false) => absolute,
        (Err(relative), true) => cwd_opt?.join_checked(relative.canonical()),
        (Err(relative), false) => cwd_opt?.join_checked(relative),
    };
    match absolute.canonical() {
        RedoxPath::Standard(redox_reference) => Some(redox_reference.0),
        RedoxPath::Legacy(redox_scheme, redox_reference) => {
            Some(format!("{redox_scheme}:{redox_reference}").into())
        }
    }
}

/// Canonicalize as in [`canonicalize_using_cwd`], but Legacy format paths
/// are canonicalized into UNIX format
pub fn canonicalize_to_standard<'a>(
    cwd_opt: Option<&'a str>,
    path: &'a str,
) -> Option<Cow<'a, str>> {
    Some(match RedoxPath::from_absolute(path) {
        Some(absolute) => absolute.to_standard_canon(),
        None => {
            let absolute = RedoxPath::from_absolute(cwd_opt?)?;
            absolute.join(path)?.to_standard_canon()
        }
    })
}

/// [`canonicalize_to_standard`] but for internal use.
pub fn canonicalize_to_standard_checked<'a, 'b>(
    cwd_opt: Option<&RedoxPath<'a>>,
    path: RedoxPathStr<'b>,
) -> Option<Cow<'b, str>> {
    Some(
        match path {
            Ok(absolute) => absolute,
            Err(relative) => cwd_opt?.join_checked(relative),
        }
        .to_standard_canon(),
    )
}

/// Make a path that is relative to the root of a scheme into a full path,
/// following the rules of [`canonicalize_using_cwd`].
///
/// Returns the Some if a valid path can be constructed,
/// None if the scheme name is not valid or some other error occurs.
/// The returned value is guaranteed to be standard, but not guaranteed to be the given scheme.
pub fn canonicalize_using_scheme<'a>(scheme: &'a str, path: &'a str) -> Option<Cow<'a, str>> {
    let path = RedoxPath::from_absolute_checked(path)?;
    Some(canonicalize_using_scheme_checked(
        &RedoxScheme::new(scheme)?,
        path,
    ))
}

/// [`canonicalize_using_scheme`] but for internal use.
fn canonicalize_using_scheme_checked<'a, 'b>(
    scheme: &RedoxScheme<'_>,
    path: RedoxPathStr<'b>,
) -> Cow<'b, str> {
    let path = match path {
        Ok(s) => {
            if s.matches_scheme(&scheme.0) {
                // Canonicalized and being in the same scheme, return early
                // to avoid allocation from scheme_path_checked
                return s.to_standard();
            }
            Ok(s)
        }
        path => path,
    };
    // SAFETY: canonicalize_using_cwd_checked will only None if cwd_opt is None
    canonicalize_using_cwd_checked(Some(&scheme_path_checked(&scheme)), path, true).unwrap()
}

/// The standard path for a given scheme name
///
/// Returns Some if the scheme name is valid, None otherwise
pub fn scheme_path(name: &str) -> Option<RedoxPath<'_>> {
    let scheme = RedoxScheme::new(name)?;
    Some(scheme_path_checked(&scheme))
}

/// [`scheme_path`] but for internal use.
fn scheme_path_checked<'a, 'b>(scheme: &'a RedoxScheme<'a>) -> RedoxPath<'b> {
    if scheme.0.is_empty() {
        return RedoxPath::Standard(RedoxReference("/scheme".into()));
    }
    RedoxPath::Standard(RedoxReference(format!("/scheme/{}", scheme.0).into()))
}

/// Make a scheme name (not a full path) for a device of a given category
///
/// Returns Some if the resulting scheme name is valid, None otherwise
pub fn make_scheme_name<'a, 'b>(category: &'a str, detail: &'a str) -> Option<RedoxScheme<'b>> {
    let name = format!("{}.{}", category, detail);
    RedoxScheme::new(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    // Tests absolute paths without scheme
    #[test]
    fn test_absolute() {
        let cwd_opt = None;
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/"),
            Some(Cow::Borrowed("/"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/file"),
            Some(Cow::Borrowed("/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/file"),
            Some(Cow::Borrowed("/folder/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/file/"),
            Some(Cow::Borrowed("/folder/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/file//"),
            Some(Cow::Borrowed("/folder/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/../file"),
            Some(Cow::Borrowed("/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/../.."),
            Some(Cow::Borrowed("/"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/folder/../../../.."),
            Some(Cow::Borrowed("/"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/.."),
            Some(Cow::Borrowed("/"))
        );
    }

    // Test relative paths using new scheme
    #[test]
    fn test_new_relative() {
        let cwd_opt = Some("/scheme/foo");
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "file"),
            Some(Cow::Borrowed("/scheme/foo/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/file"),
            Some(Cow::Borrowed("/scheme/foo/folder/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../file"),
            Some(Cow::Borrowed("/scheme/foo/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../.."),
            Some(Cow::Borrowed("/scheme"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../../../.."),
            Some(Cow::Borrowed("/"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, ".."),
            Some(Cow::Borrowed("/scheme"))
        );
    }

    // Tests paths prefixed with /scheme/
    #[test]
    fn test_new_scheme() {
        let cwd_opt = None;
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/"),
            Some(Cow::Borrowed("/scheme/bar"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/file"),
            Some(Cow::Borrowed("/scheme/bar/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/file"),
            Some(Cow::Borrowed("/scheme/bar/folder/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/../file"),
            Some(Cow::Borrowed("/scheme/bar/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/../.."),
            Some(Cow::Borrowed("/scheme"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/folder/../../../.."),
            Some(Cow::Borrowed("/"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "/scheme/bar/.."),
            Some(Cow::Borrowed("/scheme"))
        );

        assert_eq!(
            canonicalize_using_scheme("bar", ""),
            Some(Cow::Borrowed("/scheme/bar"))
        );
        assert_eq!(
            canonicalize_using_scheme("bar", "foo"),
            Some(Cow::Borrowed("/scheme/bar/foo"))
        );
        assert_eq!(
            canonicalize_using_scheme("foo", "/bar"),
            Some(Cow::Borrowed("/scheme/foo/bar"))
        );
        assert_eq!(
            canonicalize_using_scheme("bar", ".."),
            Some(Cow::Borrowed("/scheme/bar"))
        );
        assert_eq!(
            canonicalize_using_scheme("file", "/scheme/foo/bar"),
            Some(Cow::Borrowed("/scheme/foo/bar"))
        );
    }

    // Test relative paths using old scheme
    #[test]
    fn test_old_relative() {
        let cwd_opt = Some("foo:");
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "file"),
            Some(Cow::Borrowed("foo:file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/file"),
            Some(Cow::Borrowed("foo:folder/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../file"),
            Some(Cow::Borrowed("foo:folder/../file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../.."),
            Some(Cow::Borrowed("foo:folder/../.."))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "folder/../../../.."),
            Some(Cow::Borrowed("foo:folder/../../../.."))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, ".."),
            Some(Cow::Borrowed("foo:.."))
        );
    }

    // Tests paths prefixed with scheme_name:
    #[test]
    fn test_old_scheme() {
        let cwd_opt = None;
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:"),
            Some(Cow::Borrowed("bar:"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:file"),
            Some(Cow::Borrowed("bar:file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/file"),
            Some(Cow::Borrowed("bar:folder/file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/../file"),
            Some(Cow::Borrowed("bar:folder/../file"))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/../.."),
            Some(Cow::Borrowed("bar:folder/../.."))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:folder/../../../.."),
            Some(Cow::Borrowed("bar:folder/../../../.."))
        );
        assert_eq!(
            canonicalize_using_cwd(cwd_opt, "bar:.."),
            Some(Cow::Borrowed("bar:.."))
        );
    }

    // Test whether paths are borrowed or owned
    #[test]
    fn test_cow_borrowing() {
        let cwd_opt = None;
        for (path, should_owned) in [
            ("/foo/bar/baz", false),
            ("/scheme/foo/bar/baz", false),
            ("foo/bar/baz", false), // this is None(Cow::Borrowed)
            ("/scheme/foo/../baz", true),
            ("foo:bar", true),
        ] {
            assert_eq!(
                matches!(canonicalize_using_cwd(cwd_opt, path), Some(Cow::Owned(_))),
                should_owned
            );
        }
    }
    // Test whether dirname works
    #[test]
    fn test_dirname() {
        for (a, b) in [
            ("/foo/bar/baz", "/foo/bar"),
            ("/baz/foo/bar/", "/baz/foo"),
            ("/", "/"),
            ("/scheme/foo", "/scheme"),
            ("foo:bar/baz/", "foo:bar"),
            ("foo:bar", "foo:"),
            ("foo:", "foo:"),
        ] {
            assert_eq!(
                RedoxPath::from_absolute(a).unwrap().dirname(),
                RedoxPath::from_absolute(b).unwrap()
            );
        }
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
                                assert_eq!(
                                    canonicalize_using_cwd(None, &path),
                                    Some((&path).into())
                                );
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
        for (path, scheme, reference) in [
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
                        RedoxScheme::new(scheme).unwrap(),
                        RedoxReference::new(reference).unwrap()
                    ))
                )
            );
            if path.starts_with("/scheme") {
                let to_string = format!("/scheme/{scheme}");
                let joined_path = RedoxPath::from_absolute(&to_string)
                    .unwrap()
                    .join(reference)
                    .unwrap();
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
            "/scheme/use:colon/",
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
        for (path, scheme, reference) in [
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
                        RedoxScheme::new(scheme).unwrap(),
                        RedoxReference::new(reference).unwrap()
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
            canonicalize_using_cwd(Some("/scheme/foo"), "bar").unwrap()
        )
        .unwrap()
        .matches_scheme("foo"));
        assert!(
            RedoxPath::from_absolute(canonicalize_using_cwd(Some("/foo"), "bar").unwrap())
                .unwrap()
                .matches_scheme("file")
        );
        assert!(RedoxPath::from_absolute(
            canonicalize_using_cwd(Some("/scheme"), "foo/bar").unwrap()
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
                .to_standard_canon(),
            "/scheme/foo/bar2"
        );
        assert_eq!(
            &RedoxPath::from_absolute("file:bar/./../bar2")
                .unwrap()
                .to_standard_canon(),
            "/scheme/file/bar2"
        );
        assert_eq!(
            &RedoxPath::from_absolute("/scheme/file/bar/./../../foo/bar")
                .unwrap()
                .to_standard_canon(),
            "/scheme/foo/bar"
        );
        assert_eq!(
            &RedoxPath::from_absolute("/foo/bar")
                .unwrap()
                .to_standard_canon(),
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
        assert_eq!(
            scheme_path("foo"),
            Some(RedoxPath::from_absolute("/scheme/foo").unwrap())
        );
        assert_eq!(
            scheme_path(""),
            Some(RedoxPath::from_absolute("/scheme").unwrap())
        );
        assert_eq!(scheme_path("/foo"), None);
        assert_eq!(scheme_path("foo/bar"), None);
        assert_eq!(scheme_path("foo:"), None);
    }

    #[test]
    fn test_category() {
        assert_eq!(
            make_scheme_name("foo", "bar"),
            Some(RedoxScheme::new("foo.bar").unwrap())
        );
        assert_eq!(
            RedoxPath::from_absolute(
                scheme_path(make_scheme_name("foo", "bar").unwrap().as_ref()).unwrap()
            )
            .unwrap(),
            RedoxPath::Standard(RedoxReference::new("/scheme/foo.bar").unwrap())
        );
        assert_eq!(make_scheme_name("foo", "/bar"), None);
        assert_eq!(make_scheme_name("foo", ":bar"), None);
        assert!(RedoxPath::from_absolute(
            scheme_path(make_scheme_name("foo", "bar").unwrap().as_ref()).unwrap()
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
