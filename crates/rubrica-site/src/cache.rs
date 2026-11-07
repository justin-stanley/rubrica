//! A cache of page renders, for a server that rebuilds the site on every
//! push: a file whose content did not change is not rendered again,
//! and its render is shared with the cache, not copied.

use std::sync::Arc;

use rubrica_render::frontmatter::FrontMatterError;
use rubrica_render::{FrontMatter, Rendered};
use sha2::{Digest, Sha256};

/// What [`rubrica_render::render_page`] made of a page file: its front
/// matter, its body's render (shared: [`Arc`]) and the lines before the
/// body; or why its front matter does not parse.
pub type RenderResult = Result<(FrontMatter, Arc<Rendered>, usize), FrontMatterError>;

/// What a render is cached under: the file's content, never its path (a
/// render depends on the bytes and the site's profile, `SiteConfig.render`; a
/// cache lives for one build, under one profile, so the key needs no more
/// here; a cache that outlives a profile change, such as a live server's,
/// must key on both).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RenderKey {
    /// The git blob id the caller gave, which names the content.
    Blob(String),
    /// The SHA-256 of the bytes, when the caller gave no blob id.
    Sha256([u8; 32]),
}

impl RenderKey {
    /// The key of a file with these bytes and blob id.
    pub fn of(bytes: &[u8], blob_id: Option<&str>) -> Self {
        match blob_id {
            Some(id) => Self::Blob(id.to_owned()),
            None => Self::Sha256(Sha256::digest(bytes).into()),
        }
    }
}

/// Renders kept between builds. [`crate::Site::from_sources_with`] asks
/// for each page file's render before rendering it, and offers each one it
/// made.
pub trait RenderCache {
    /// The render cached under `key`, if any (its `Rendered` shared, not
    /// copied: clone the [`Arc`]).
    fn get(&mut self, key: &RenderKey) -> Option<RenderResult>;
    /// Offers the render of the file with `key`.
    fn put(&mut self, key: RenderKey, render: &RenderResult);
}

/// No cache: every page is rendered.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCache;

impl RenderCache for NoCache {
    fn get(&mut self, _: &RenderKey) -> Option<RenderResult> {
        None
    }
    fn put(&mut self, _: RenderKey, _: &RenderResult) {}
}
