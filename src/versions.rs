use std::fmt;
use std::str::FromStr;

use anyhow::{Context, Result, bail};

/// A fully-resolved runtime version, e.g. `3.12.8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl FromStr for Version {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parts = s.split('.');
        let mut next = |what| {
            parts
                .next()
                .with_context(|| format!("missing {what} in version '{s}'"))?
                .parse::<u32>()
                .with_context(|| format!("invalid {what} in version '{s}'"))
        };
        let version = Version {
            major: next("major")?,
            minor: next("minor")?,
            patch: next("patch")?,
        };
        if parts.next().is_some() {
            bail!("too many components in version '{s}'");
        }
        Ok(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A version request as written by a user or pin file: `3`, `3.12`, or `3.12.8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionReq {
    Major(u32),
    MajorMinor(u32, u32),
    Exact(Version),
}

impl FromStr for VersionReq {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        let num = |p: &str| {
            p.parse::<u32>()
                .with_context(|| format!("invalid version request '{s}'"))
        };
        match parts.as_slice() {
            [maj] => Ok(VersionReq::Major(num(maj)?)),
            [maj, min] => Ok(VersionReq::MajorMinor(num(maj)?, num(min)?)),
            [_, _, _] => Ok(VersionReq::Exact(s.parse()?)),
            _ => bail!("invalid version request '{s}'"),
        }
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionReq::Major(maj) => write!(f, "{maj}"),
            VersionReq::MajorMinor(maj, min) => write!(f, "{maj}.{min}"),
            VersionReq::Exact(v) => write!(f, "{v}"),
        }
    }
}

impl VersionReq {
    pub fn matches(&self, v: &Version) -> bool {
        match *self {
            VersionReq::Major(maj) => v.major == maj,
            VersionReq::MajorMinor(maj, min) => v.major == maj && v.minor == min,
            VersionReq::Exact(exact) => *v == exact,
        }
    }

    /// The highest version among `candidates` that satisfies this request.
    pub fn best_match(&self, candidates: impl IntoIterator<Item = Version>) -> Option<Version> {
        candidates.into_iter().filter(|v| self.matches(v)).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    fn req(s: &str) -> VersionReq {
        s.parse().unwrap()
    }

    #[test]
    fn parses_versions() {
        assert_eq!(
            v("3.12.8"),
            Version {
                major: 3,
                minor: 12,
                patch: 8
            }
        );
        assert!("3.12".parse::<Version>().is_err());
        assert!("3.12.8.1".parse::<Version>().is_err());
        assert!("3.x.1".parse::<Version>().is_err());
    }

    #[test]
    fn parses_requests() {
        assert_eq!(req("3"), VersionReq::Major(3));
        assert_eq!(req("3.12"), VersionReq::MajorMinor(3, 12));
        assert_eq!(req("3.12.8"), VersionReq::Exact(v("3.12.8")));
        assert!("".parse::<VersionReq>().is_err());
        assert!("3.12.8.1".parse::<VersionReq>().is_err());
    }

    #[test]
    fn matching() {
        assert!(req("3").matches(&v("3.12.8")));
        assert!(req("3.12").matches(&v("3.12.8")));
        assert!(!req("3.11").matches(&v("3.12.8")));
        assert!(req("3.12.8").matches(&v("3.12.8")));
        assert!(!req("3.12.8").matches(&v("3.12.9")));
    }

    #[test]
    fn best_match_picks_highest() {
        let installed = vec![v("3.11.9"), v("3.12.3"), v("3.12.8"), v("3.13.1")];
        assert_eq!(req("3.12").best_match(installed.clone()), Some(v("3.12.8")));
        assert_eq!(req("3").best_match(installed.clone()), Some(v("3.13.1")));
        assert_eq!(req("3.10").best_match(installed), None);
    }
}
