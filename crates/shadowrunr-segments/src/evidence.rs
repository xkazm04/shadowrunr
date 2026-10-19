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

/// The compiled-in research allowlist: the exact bytes of `data/research-allowlist.toml`.
pub const BUILTIN_RESEARCH_ALLOWLIST_TOML: &str = include_str!("../data/research-allowlist.toml");

/// Domains the off-stream research pipeline may fetch from. It governs network egress only;
/// grades stay with [`SourceAllowlist`] (a page fetched from `web.archive.org` is never a
/// primary source by its own host).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchAllowlist {
    pub domains: Vec<String>,
}

impl ResearchAllowlist {
    pub fn from_toml_str(text: &str) -> Result<Self, SegmentError> {
        let list: ResearchAllowlist = toml::from_str(text).map_err(|e| SegmentError::Data {
            file: "research-allowlist.toml".into(),
            detail: e.to_string(),
        })?;
        Self::from_domains(list.domains)
    }

    /// A list from bare domains (`sec.gov`), normalised like the primary allowlist. Empty, a
    /// path or a dotless name is refused.
    pub fn from_domains(domains: Vec<String>) -> Result<Self, SegmentError> {
        let mut out = Vec::with_capacity(domains.len());
        for d in domains {
            let mut d = d.trim().to_ascii_lowercase();
            if let Some(bare) = d.strip_prefix("www.") {
                d = bare.to_string();
            }
            if d.is_empty() || d.contains('/') || d.contains(':') || !d.contains('.') {
                return Err(SegmentError::Data {
                    file: "research-allowlist.toml".into(),
                    detail: format!("not a bare domain: {d:?}"),
                });
            }
            if !out.contains(&d) {
                out.push(d);
            }
        }
        if out.is_empty() {
            return Err(SegmentError::Data {
                file: "research-allowlist.toml".into(),
                detail: "the research allowlist is empty".into(),
            });
        }
        Ok(Self { domains: out })
    }

    pub fn builtin() -> &'static ResearchAllowlist {
        static LIST: OnceLock<ResearchAllowlist> = OnceLock::new();
        LIST.get_or_init(|| {
            ResearchAllowlist::from_toml_str(BUILTIN_RESEARCH_ALLOWLIST_TOML)
                .expect("built-in research-allowlist.toml is valid (checked by tests)")
        })
    }

    /// May the pipeline fetch this URL? Only `https://` URLs on a listed domain (or a
    /// subdomain of one) pass.
    pub fn allows(&self, url: &str) -> bool {
        if !url.trim().to_ascii_lowercase().starts_with("https://") {
            return false;
        }
        url_host(url).is_some_and(|host| {
            self.domains
                .iter()
                .any(|d| host == *d || host.ends_with(&format!(".{d}")))
        })
    }
}

impl Default for ResearchAllowlist {
    fn default() -> Self {
        Self::builtin().clone()
    }
}

/// The original URL of a Wayback Machine capture
/// (`https://web.archive.org/web/<timestamp>[id_]/<original>`), or `None` for any other URL.
/// A capture keeps its original's provenance: the record it copies was published at the
/// original URL, so that URL is the source the DOC rule reads.
pub fn wayback_original(url: &str) -> Option<String> {
    if url_host(url).as_deref() != Some("web.archive.org") {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    let at = lower.find("/web/")? + "/web/".len();
    let rest = &url[at..];
    let (stamp, original) = rest.split_once('/')?;
    let digits = stamp.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '_');
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let o = original.to_ascii_lowercase();
    (o.starts_with("https://") || o.starts_with("http://")).then(|| original.to_string())
}

/// Lowercase host of an `http(s)://` URL, without userinfo, port or a leading `www.`.
/// Anything that is not an absolute http(s) URL yields `None`.
pub(crate) fn url_host(url: &str) -> Option<String> {
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
    fn research_allowlist_is_the_owner_list() {
        let list = ResearchAllowlist::builtin();
        assert_eq!(
            list.domains,
            [
                "ftc.gov",
                "sec.gov",
                "fca.org.uk",
                "dfpi.ca.gov",
                "justice.gov",
                "opencorporates.com",
                "web.archive.org"
            ]
        );
        for u in [
            "https://www.sec.gov/litigation/litreleases/2019/lr24001.htm",
            "https://web.archive.org/web/2019id_/https://www.ftc.gov/x",
            "https://opencorporates.com/companies/us_de/123",
            "https://www.fca.org.uk/news",
        ] {
            assert!(list.allows(u), "{u}");
        }
        for u in [
            "http://www.sec.gov/x",
            "https://en.wikipedia.org/wiki/X",
            "https://sec.gov.evil.example/x",
            "https://archive.org/wayback/available?url=x",
            "https://courtlistener.com/x",
            "",
        ] {
            assert!(!list.allows(u), "{u}");
        }
        assert!(ResearchAllowlist::from_domains(vec![]).is_err());
        assert!(ResearchAllowlist::from_domains(vec!["https://sec.gov".into()]).is_err());
    }

    #[test]
    fn wayback_captures_keep_their_original() {
        assert_eq!(
            wayback_original("https://web.archive.org/web/20210104183940id_/https://www.sec.gov/news/press-release/2020-146")
                .as_deref(),
            Some("https://www.sec.gov/news/press-release/2020-146")
        );
        assert_eq!(
            wayback_original("https://web.archive.org/web/2019/http://example.com/a?b=c")
                .as_deref(),
            Some("http://example.com/a?b=c")
        );
        assert_eq!(
            wayback_original("https://www.sec.gov/web/2019/https://x.gov"),
            None
        );
        assert_eq!(
            wayback_original("https://web.archive.org/web/abc/https://x.gov"),
            None
        );
        assert_eq!(
            wayback_original("https://web.archive.org/web/2019/ftp://x"),
            None
        );
    }

    #[test]
    fn bad_domain_rejected() {
        assert!(SourceAllowlist::from_toml_str("domains = [\"https://sec.gov/\"]").is_err());
        assert!(SourceAllowlist::from_toml_str("domains = [\"localhost\"]").is_err());
        let ok = SourceAllowlist::from_toml_str("domains = [\"WWW.Example.org\"]").unwrap();
        assert!(ok.is_primary("https://example.org/a", false));
    }
}
