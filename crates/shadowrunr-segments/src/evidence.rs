//! Evidence grades, the primary-source allowlist and the DOC rule.

use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;
use shadowrunr_types::{EvidenceGrade, Hex};

use crate::SegmentError;

/// The compiled-in starter allowlist: the exact bytes of `data/primary-source-allowlist.toml`.
pub const BUILTIN_ALLOWLIST_TOML: &str = include_str!("../data/primary-source-allowlist.toml");

/// True when a statement at this grade must carry an "alleged" qualifier: REP, CLM and SPC.
/// Only DOC (documented by an allowlisted primary source) may be stated as fact. For the
/// brain's output gate.
pub fn requires_alleged(grade: EvidenceGrade) -> bool {
    !matches!(grade, EvidenceGrade::Doc)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FictionalSection {
    #[serde(default)]
    domains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceAllowlist {
    /// Regulator and court domains; a source on one of these (or a subdomain) is primary.
    pub domains: Vec<String>,
    /// Reserved example domains honoured only for `fictional: true` cases.
    #[serde(default)]
    fictional: FictionalSection,
}

impl SourceAllowlist {
    pub fn from_toml_str(text: &str) -> Result<Self, SegmentError> {
        let mut list: SourceAllowlist = toml::from_str(text).map_err(|e| SegmentError::Data {
            file: "primary-source-allowlist.toml".into(),
            detail: e.to_string(),
        })?;
        for d in list
            .domains
            .iter_mut()
            .chain(list.fictional.domains.iter_mut())
        {
            *d = d.trim().to_ascii_lowercase();
            if let Some(bare) = d.strip_prefix("www.") {
                *d = bare.to_string();
            }
            if d.is_empty() || d.contains('/') || !d.contains('.') {
                return Err(SegmentError::Data {
                    file: "primary-source-allowlist.toml".into(),
                    detail: format!("not a bare domain: {d:?}"),
                });
            }
        }
        Ok(list)
    }

    pub fn load(path: &Path) -> Result<Self, SegmentError> {
        let text = std::fs::read_to_string(path).map_err(|e| SegmentError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        Self::from_toml_str(&text)
    }

    pub fn builtin() -> &'static SourceAllowlist {
        static LIST: OnceLock<SourceAllowlist> = OnceLock::new();
        LIST.get_or_init(|| {
            SourceAllowlist::from_toml_str(BUILTIN_ALLOWLIST_TOML)
                .expect("built-in primary-source-allowlist.toml is valid (checked by tests)")
        })
    }

    /// Is this source URL on the allowlist? `fictional_case` additionally admits the reserved
    /// example domains.
    pub fn is_primary(&self, url: &str, fictional_case: bool) -> bool {
        let Some(host) = url_host(url) else {
            return false;
        };
        let fictional = if fictional_case {
            self.fictional.domains.as_slice()
        } else {
            &[]
        };
        self.domains
            .iter()
            .chain(fictional)
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    }

    /// DOC rule: true when the hex carries at least one allowlisted primary source.
    pub fn hex_has_primary_source(&self, hex: &Hex, fictional_case: bool) -> bool {
        hex.sources
            .iter()
            .any(|s| self.is_primary(s, fictional_case))
    }
}

impl Default for SourceAllowlist {
    fn default() -> Self {
        Self::builtin().clone()
    }
}

/// Lowercase host of an `http(s)://` URL, without userinfo, port or a leading `www.`.
/// Anything that is not an absolute http(s) URL yields `None`.
fn url_host(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#', '\\']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = host_port.split(':').next()?.trim_end_matches('.');
    let host = host.strip_prefix("www.").unwrap_or(host);
    let valid = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    valid.then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alleged_below_doc() {
        assert!(!requires_alleged(EvidenceGrade::Doc));
        assert!(requires_alleged(EvidenceGrade::Rep));
        assert!(requires_alleged(EvidenceGrade::Clm));
        assert!(requires_alleged(EvidenceGrade::Spc));
    }

    #[test]
    fn builtin_has_starter_domains() {
        let list = SourceAllowlist::builtin();
        for d in [
            "sec.gov",
            "ftc.gov",
            "fca.org.uk",
            "courtlistener.com",
            "justice.gov",
        ] {
            assert!(list.domains.iter().any(|x| x == d), "{d}");
        }
    }

    #[test]
    fn primary_matching() {
        let list = SourceAllowlist::builtin();
        let yes = [
            "https://www.sec.gov/litigation/complaints/2026/x.pdf",
            "https://SEC.GOV/x",
            "http://efts.sec.gov/LATEST/search",
            "https://www.ftc.gov/news-events",
            "https://www.courtlistener.com/docket/1/x/",
            "https://sec.gov:443/x",
            "https://sec.gov",
        ];
        for u in yes {
            assert!(list.is_primary(u, false), "{u}");
        }
        let no = [
            "https://sec.gov.evil.example/x",
            "https://notsec.gov/x",
            "https://sec.gov@evil.example/x",
            "https://evil.example/?u=https://sec.gov",
            "ftp://sec.gov/x",
            "sec.gov/x",
            "https://news.example/sec.gov",
            "",
            "https://records.example/x",
        ];
        for u in no {
            assert!(!list.is_primary(u, false), "{u}");
        }
    }

    #[test]
    fn fictional_domains_only_for_fictional_cases() {
        let list = SourceAllowlist::builtin();
        let u = "https://records.example/morrowgate-fco/order-0001";
        assert!(!list.is_primary(u, false));
        assert!(list.is_primary(u, true));
    }

    #[test]
    fn bad_domain_rejected() {
        assert!(SourceAllowlist::from_toml_str("domains = [\"https://sec.gov/\"]").is_err());
        assert!(SourceAllowlist::from_toml_str("domains = [\"localhost\"]").is_err());
        let ok = SourceAllowlist::from_toml_str("domains = [\"WWW.Example.org\"]").unwrap();
        assert!(ok.is_primary("https://example.org/a", false));
    }
}
