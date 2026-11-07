//! What a build is told beyond the repository: the site's name and links,
//! and the changelog feed. [`BuildConfig::default`] is a neutral site;
//! `rubrica build --config <file>` reads one in `site/site.toml`'s shape
//! ([`ConfigFile`], `site/DESIGN.md` on the `site/launch-page` branch).
//!
//! The values are the operator's, not a page author's, but they are still
//! checked ([`BuildConfig::validate`]): every URL is `https://` or
//! root-relative, so no configuration can write a `javascript:` link.

use serde::Deserialize;

use crate::{Origin, SiteConfig};

/// A link in the site header.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderLink {
    /// Its text.
    pub label: String,
    /// Root-relative (`/docs`) or `https://`.
    pub href: String,
}

/// The footer's facts (`site.toml`'s `version`, `license`, `source_url`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Footer {
    /// The release the site describes (`0.1`).
    pub version: Option<String>,
    /// Its licence (`AGPL-3.0-only`).
    pub license: Option<String>,
    /// Its source (`https://…`).
    pub source_url: Option<String>,
}

/// The changelog's Atom feed (`site/DESIGN.md`, "The changelog feed").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedConfig {
    /// Where it is written and linked from: root-relative, one or more
    /// segments of `[A-Za-z0-9._-]`, ending in `.xml`, not under `_`
    /// (`/changelog.xml`).
    pub url: String,
    /// The page file it is made from (`changelog.md`): one entry per `##`.
    pub source: String,
}

/// What a build is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildConfig {
    /// The site's name: the wordmark and the `<title>` suffix.
    pub name: String,
    /// `<html lang>`.
    pub lang: String,
    /// The site's origin, no trailing slash, for `<link rel="canonical">`
    /// and the feed's absolute URLs.
    pub base_url: Option<String>,
    /// A page's "Edit" link: this, then its file's path
    /// (`…/_edit/main/` + `guides/setup.md`).
    pub edit_url: Option<String>,
    /// A page's "History" link: this, then its file's path.
    pub history_url: Option<String>,
    /// The header's links, after the wordmark.
    pub header_links: Vec<HeaderLink>,
    /// The footer.
    pub footer: Footer,
    /// The feed, if any.
    pub feed: Option<FeedConfig>,
}

impl Default for BuildConfig {
    /// Neutral: a site called "Wiki" in English, no origin, no edit or
    /// history links, "Recent changes" and "Search" in the header, no
    /// footer facts, no feed.
    fn default() -> Self {
        Self {
            name: "Wiki".into(),
            lang: "en".into(),
            base_url: None,
            edit_url: None,
            history_url: None,
            header_links: vec![
                HeaderLink {
                    label: "Recent changes".into(),
                    href: "/_recent".into(),
                },
                HeaderLink {
                    label: "Search".into(),
                    href: "/_search".into(),
                },
            ],
            footer: Footer::default(),
            feed: None,
        }
    }
}

impl BuildConfig {
    /// Whether every value is one the build can write safely: a non-empty
    /// name; `base_url` an `https://` origin with no trailing slash; the
    /// edit and history prefixes `https://`; header links root-relative or
    /// `https://`; a feed URL of the shape [`FeedConfig::url`] says and a
    /// feed source ending in `.md`, with a `base_url` to make its IRIs
    /// absolute. No value may hold a control character, a quote, `<`, `>`
    /// or a blank.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("name is empty".into());
        }
        if self.lang.is_empty()
            || !self
                .lang
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(format!("lang {:?} is not a language tag", self.lang));
        }
        if let Some(b) = &self.base_url {
            url_ok("base_url", b, Kind::Https)?;
            if b.ends_with('/') || b["https://".len()..].contains('/') {
                return Err(format!(
                    "base_url {b:?} must be an origin, with no path or trailing slash"
                ));
            }
        }
        for (key, v) in [
            ("edit_url", &self.edit_url),
            ("history_url", &self.history_url),
        ] {
            if let Some(v) = v {
                url_ok(key, v, Kind::Https)?;
            }
        }
        if let Some(v) = &self.footer.source_url {
            url_ok("source_url", v, Kind::Https)?;
        }
        for l in &self.header_links {
            url_ok("header_links.href", &l.href, Kind::HttpsOrRoot)?;
        }
        if let Some(f) = &self.feed {
            let path = f
                .url
                .strip_prefix('/')
                .filter(|p| p.ends_with(".xml") && !p.starts_with('_'))
                .ok_or_else(|| format!("feed_url {:?} must be /<name>.xml, not under _", f.url))?;
            if !path.split('/').all(|s| {
                !s.is_empty()
                    && s != "."
                    && s != ".."
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            }) {
                return Err(format!(
                    "feed_url {:?} must be segments of [A-Za-z0-9._-]",
                    f.url
                ));
            }
            if !f.source.ends_with(".md") {
                return Err(format!(
                    "feed_source {:?} must be a page's .md file",
                    f.source
                ));
            }
            if self.base_url.is_none() {
                return Err("a feed needs base_url, for its absolute ids and links".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Https,
    HttpsOrRoot,
}

fn url_ok(key: &str, v: &str, kind: Kind) -> Result<(), String> {
    let https = v.starts_with("https://") && v.len() > "https://".len();
    let root = v.starts_with('/') && !v.starts_with("//") && !v.starts_with("/\\");
    let shape = match kind {
        Kind::Https => https,
        Kind::HttpsOrRoot => https || root,
    };
    let clean = !v.chars().any(|c| {
        c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`')
    });
    if shape && clean {
        Ok(())
    } else {
        Err(format!(
            "{key} {v:?} must be {}, with no blank, control character, quote, < or >",
            match kind {
                Kind::Https => "an https:// URL",
                Kind::HttpsOrRoot => "an https:// URL or root-relative (/x)",
            }
        ))
    }
}

/// `site.toml`'s shape (`site/DESIGN.md`), plus `edit_url` and
/// `history_url`. Every key but `name` is optional; an unknown key is an
/// error.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    /// [`BuildConfig::name`].
    pub name: String,
    /// [`BuildConfig::base_url`].
    pub base_url: Option<String>,
    /// [`BuildConfig::lang`] (`en` when absent).
    pub lang: Option<String>,
    /// [`Footer::version`].
    pub version: Option<String>,
    /// [`Footer::license`].
    pub license: Option<String>,
    /// [`Footer::source_url`].
    pub source_url: Option<String>,
    /// [`FeedConfig::url`]; with `feed_source`, or neither.
    pub feed_url: Option<String>,
    /// [`FeedConfig::source`].
    pub feed_source: Option<String>,
    /// [`BuildConfig::edit_url`].
    pub edit_url: Option<String>,
    /// [`BuildConfig::history_url`].
    pub history_url: Option<String>,
    /// [`BuildConfig::header_links`].
    #[serde(default)]
    pub header_links: Vec<HeaderLink>,
    /// `[site]`: which files are pages and how they render.
    pub site: Option<SiteSection>,
}

/// `[site]`: which files are pages and how they render (`SiteConfig`). A key
/// that is absent is that field of `SiteConfig::default()`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteSection {
    /// [`SiteConfig::ignore_dirs`].
    pub ignore_dirs: Option<Vec<String>>,
    /// [`SiteConfig::ignore_files`].
    pub ignore_files: Option<Vec<String>>,
    /// [`SiteConfig::wiki_origin`], checked into an [`Origin`].
    pub wiki_origin: Option<String>,
    /// [`SiteConfig::render`].
    pub render: Option<RenderName>,
    /// [`SiteConfig::wikijs_limits`].
    pub wikijs_limits: Option<bool>,
}

/// The two profile names `[site] render` takes. An unknown one is serde's
/// own error, which names both: ``unknown variant `markdown-it`, expected
/// `native` or `wikijs-compat```.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderName {
    /// The native rules.
    Native,
    /// The compatibility profile.
    WikijsCompat,
}

impl From<RenderName> for rubrica_render::Profile {
    fn from(n: RenderName) -> Self {
        match n {
            RenderName::Native => Self::Native,
            RenderName::WikijsCompat => Self::WikijsCompat,
        }
    }
}

impl TryFrom<ConfigFile> for BuildConfig {
    type Error = String;

    fn try_from(f: ConfigFile) -> Result<Self, String> {
        f.into_configs().map(|(build, _)| build)
    }
}

impl ConfigFile {
    /// Both halves of one file: the build's and the site's. A `None` key is
    /// that field of `SiteConfig::default()`; an absent `[site]` is
    /// `SiteConfig::default()` whole.
    pub fn into_configs(self) -> Result<(BuildConfig, SiteConfig), String> {
        let f = self;
        let mut site_cfg = SiteConfig::default();
        if let Some(s) = f.site {
            if let Some(v) = s.ignore_dirs {
                site_cfg.ignore_dirs = v;
            }
            if let Some(v) = s.ignore_files {
                site_cfg.ignore_files = v;
            }
            if let Some(o) = s.wiki_origin {
                let origin = Origin::new(&o).map_err(|e| {
                    format!("[site] wiki_origin: {o:?} is not an http(s) origin ({e:?})")
                })?;
                site_cfg.wiki_origin = Some(origin);
            }
            if let Some(r) = s.render {
                site_cfg.render = r.into();
            }
            if let Some(v) = s.wikijs_limits {
                site_cfg.wikijs_limits = v;
            }
        }
        let feed = match (f.feed_url, f.feed_source) {
            (Some(url), Some(source)) => Some(FeedConfig { url, source }),
            (None, None) => None,
            _ => return Err("feed_url and feed_source go together".into()),
        };
        let cfg = BuildConfig {
            name: f.name,
            lang: f.lang.unwrap_or_else(|| "en".into()),
            base_url: f.base_url,
            edit_url: f.edit_url,
            history_url: f.history_url,
            header_links: f.header_links,
            footer: Footer {
                version: f.version,
                license: f.license,
                source_url: f.source_url,
            },
            feed,
        };
        cfg.validate()?;
        Ok((cfg, site_cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::BuildConfig;

    #[test]
    fn the_default_build_config_validates_and_links_nowhere() {
        let cfg = BuildConfig::default();
        cfg.validate().expect("the default validates");
        assert!(cfg.edit_url.is_none());
        assert!(cfg.history_url.is_none());
    }
}
