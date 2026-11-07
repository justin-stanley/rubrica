//! Nothing Rubrica builds can run script: an auditor over the files of a built site, run on
//! the project's own documentation site, on a hostile synthetic site, on a site made of
//! every specification example, and on any directory named by `RUBRICA_NO_JS_SITE`.
//!
//! The auditor tokenizes each HTML page with html5ever, so it sees what a browser's
//! tokenizer sees (entities decoded, names lowercased), not what a text search would.
//!
//! Two modes, by the `search-wasm` feature (on by default). With it (v2), the one script any
//! page may carry is the search bundle's loader, `<script type="module"
//! src="/_assets/rubrica-search.js"></script>`, on `_search.html` alone: its file's sha256 is
//! the bundle crate's, every `.wasm` is the bundle's bytes, both are within budget, and the
//! Content-Security-Policy follows the tag (`script-src 'self' 'wasm-unsafe-eval'; connect-src
//! 'self'` with it, `script-src 'none'; connect-src 'none'` without). Without the feature
//! (v1), no page names a script and no script file exists. Every other rule is the same in
//! both modes.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use rubrica_site::build::{self, BuildConfig, Commit, History};
use rubrica_site::{Site, SiteConfig, SourceFile};

// ---------- the auditor ----------

/// Extensions of files that run as script (or, for `wasm`, as code).
const SCRIPT_EXTENSIONS: [&str; 4] = ["js", "mjs", "cjs", "wasm"];

/// Attributes whose value is a URL (or, for `srcdoc`, a document).
const URL_ATTRIBUTES: [&str; 9] = [
    "href",
    "src",
    "action",
    "formaction",
    "srcdoc",
    "data",
    "poster",
    "background",
    "xlink:href",
];

/// The loader's path in a built site, and the module it loads beside it.
const LOADER_PATH: &str = "_assets/rubrica-search.js";
const WASM_PATH: &str = "_assets/rubrica-search_bg.wasm";
/// The only page that may name the loader.
const SEARCH_PAGE: &str = "_search.html";
/// The budget of the bundle's two files.
const WASM_BUDGET: usize = 1_572_864;
const LOADER_BUDGET: usize = 32_768;

/// What v2 knows of the bundle: the loader's digest and the module's bytes. `None` is v1: no
/// script is allowed anywhere.
struct Bundle {
    loader_sha256: String,
    wasm: &'static [u8],
}

#[cfg(feature = "search-wasm")]
fn bundle() -> Option<Bundle> {
    static MODE: std::sync::Once = std::sync::Once::new();
    MODE.call_once(|| eprintln!("no_js: mode v2 (the search bundle's loader is allowed)"));
    Some(Bundle {
        loader_sha256: sha256_hex(rubrica_wasm_bundle::LOADER.as_bytes()),
        wasm: rubrica_wasm_bundle::WASM,
    })
}

#[cfg(not(feature = "search-wasm"))]
fn bundle() -> Option<Bundle> {
    static MODE: std::sync::Once = std::sync::Once::new();
    MODE.call_once(|| eprintln!("no_js: mode v1 (no script anywhere; built without search-wasm)"));
    None
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Elements v1 refuses outright: the theme uses none of them. (Not `svg`: the theme's logo
/// is an inline one. A script inside an `svg` is a `script` tag, and is found as one.)
const REFUSED_ELEMENTS: [&str; 5] = ["base", "object", "embed", "iframe", "math"];

/// Image types a `data:` URL may carry.
const DATA_IMAGES: [&str; 4] = ["png", "jpeg", "gif", "webp"];

/// Whether `value`, as a browser reads a URL (ASCII whitespace and control characters
/// dropped, any case), names a scheme that can run script or load a document.
fn is_script_url(value: &str) -> bool {
    let v: String = value
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_ascii_control())
        .collect::<String>()
        .to_ascii_lowercase();
    if v.starts_with("javascript:") || v.starts_with("vbscript:") {
        return true;
    }
    match v.strip_prefix("data:") {
        None => false,
        Some(rest) => !DATA_IMAGES.iter().any(|t| {
            rest.strip_prefix("image/")
                .and_then(|r| r.strip_prefix(t))
                .is_some_and(|r| r.starts_with(';') || r.starts_with(','))
        }),
    }
}

/// Whether a refresh's `content` is exactly what a redirect stub writes: `0; url=` and a
/// path of the same site (one `/` first, not `//` or `/\\`, which a browser reads as another
/// host), with no whitespace, control character or script URL.
fn is_site_redirect(content: &str) -> bool {
    let Some(target) = content.strip_prefix("0; url=") else {
        return false;
    };
    target.starts_with('/')
        && !target.starts_with("//")
        && !target.contains(|c: char| c == '\\' || c.is_whitespace() || c.is_control())
        && !is_script_url(target)
}

/// Collects the problems of one page's tokens.
struct Audit<'a> {
    path: &'a str,
    /// An HTML page (the policy meta and the refused elements apply); else an SVG file.
    html: bool,
    /// Whether the loader's script tag is allowed at all (v2).
    loader_allowed: bool,
    problems: RefCell<Vec<String>>,
    /// The `content` of each Content-Security-Policy meta.
    csp: RefCell<Vec<String>>,
    /// How many tags are exactly the loader's.
    loaders: Cell<usize>,
    /// A loader tag was just opened: the next token must be its end tag.
    open_loader: Cell<bool>,
}

/// Whether a script tag is exactly the loader's: `type="module"` and `src` the loader's path,
/// no other attribute, not self-closing.
fn is_loader_tag(tag: &html5ever::tokenizer::Tag) -> bool {
    let attrs: BTreeMap<&str, &str> = tag
        .attrs
        .iter()
        .map(|a| (&*a.name.local, &*a.value))
        .collect();
    !tag.self_closing
        && tag.attrs.len() == 2
        && attrs.get("type") == Some(&"module")
        && attrs.get("src").copied() == Some(&format!("/{LOADER_PATH}")[..])
}

impl TokenSink for Audit<'_> {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let path = self.path;
        if self.open_loader.replace(false) {
            // Nothing between the loader's tags: no text, no comment, no other tag.
            let closes = matches!(&token, Token::TagToken(t)
                if t.kind == TagKind::EndTag && &*t.name == "script");
            if !closes {
                self.problems
                    .borrow_mut()
                    .push(format!("content inside the loader script tag in {path}"));
            }
        }
        let Token::TagToken(tag) = token else {
            return TokenSinkResult::Continue;
        };
        if tag.kind != TagKind::StartTag {
            return TokenSinkResult::Continue;
        }
        let name = &*tag.name;
        let mut problems = self.problems.borrow_mut();
        if name == "script" {
            if self.loader_allowed && self.html && is_loader_tag(&tag) {
                self.loaders.set(self.loaders.get() + 1);
                self.open_loader.set(true);
            } else {
                problems.push(format!("script tag in {path}"));
            }
        } else if self.html && REFUSED_ELEMENTS.contains(&name) {
            problems.push(format!("<{name}> element in {path}"));
        }
        let mut http_equiv = None;
        let mut content = None;
        for attr in &tag.attrs {
            let key = &*attr.name.local;
            let value = &*attr.value;
            if key.starts_with("on") {
                problems.push(format!("{key} attribute on <{name}> in {path}"));
            }
            if URL_ATTRIBUTES.contains(&key) && is_script_url(value) {
                problems.push(format!(
                    "{key} attribute on <{name}> in {path} is a script URL"
                ));
            }
            if key == "http-equiv" {
                http_equiv = Some(value.trim().to_ascii_lowercase());
            } else if key == "content" {
                content = Some(value);
            }
        }
        if name == "meta" {
            match http_equiv.as_deref() {
                Some("refresh") if is_site_redirect(content.unwrap_or("")) => {}
                Some("refresh") => problems.push(format!("meta refresh in {path}")),
                Some("content-security-policy") => {
                    self.csp.borrow_mut().push(content.unwrap_or("").to_owned());
                }
                _ => {}
            }
        }
        TokenSinkResult::Continue
    }
}

/// The tokens of one directive of a policy (`None` when it has none), sorted.
fn csp_directive(csp: &str, name: &str) -> Option<Vec<String>> {
    csp.split(';').find_map(|d| {
        let mut words = d.split_ascii_whitespace();
        (words.next()?.eq_ignore_ascii_case(name)).then(|| {
            let mut v: Vec<String> = words.map(str::to_ascii_lowercase).collect();
            v.sort();
            v
        })
    })
}

/// Whether the policy's `name` directive is exactly `want` (any order).
fn csp_is(csp: &str, name: &str, want: &[&str]) -> bool {
    let mut want: Vec<String> = want.iter().map(|w| (*w).to_owned()).collect();
    want.sort();
    csp_directive(csp, name) == Some(want)
}

/// The problems of one `.html` or `.svg` file, and, for an HTML page, how many loader tags
/// it carries.
fn audit_markup(
    path: &str,
    bytes: &[u8],
    html: bool,
    loader_allowed: bool,
) -> (Vec<String>, usize) {
    let text = String::from_utf8_lossy(bytes);
    let tokenizer = Tokenizer::new(
        Audit {
            path,
            html,
            loader_allowed,
            problems: RefCell::default(),
            csp: RefCell::default(),
            loaders: Cell::new(0),
            open_loader: Cell::new(false),
        },
        TokenizerOpts::default(),
    );
    let input = BufferQueue::default();
    input.push_back(text.as_ref().into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let sink = tokenizer.sink;
    let mut problems = sink.problems.into_inner();
    if sink.open_loader.get() {
        problems.push(format!("content inside the loader script tag in {path}"));
    }
    let loaders = sink.loaders.get();
    if html {
        if loaders > 1 {
            problems.push(format!("{loaders} loader script tags in {path}"));
        }
        if loaders > 0 && path != SEARCH_PAGE {
            problems.push(format!(
                "the loader script tag is on {path}; only {SEARCH_PAGE} may carry it"
            ));
        }
        let csp = sink.csp.into_inner();
        match csp.as_slice() {
            [one] if loaders > 0 => {
                if !csp_is(one, "script-src", &["'self'", "'wasm-unsafe-eval'"]) {
                    problems.push(format!(
                        "the CSP meta in {path} lacks script-src 'self' 'wasm-unsafe-eval'"
                    ));
                }
                if !csp_is(one, "connect-src", &["'self'"]) {
                    problems.push(format!("the CSP meta in {path} lacks connect-src 'self'"));
                }
            }
            [one] => {
                if !csp_is(one, "script-src", &["'none'"]) {
                    problems.push(format!("the CSP meta in {path} lacks script-src 'none'"));
                }
                if !csp_is(one, "connect-src", &["'none'"]) {
                    problems.push(format!("the CSP meta in {path} lacks connect-src 'none'"));
                }
            }
            many => problems.push(format!(
                "{} CSP metas in {path}, expected exactly one",
                many.len()
            )),
        }
    }
    (problems, loaders)
}

/// Every problem in the files of a site: `(path relative to the site, bytes)` pairs.
fn audit_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Vec<String> {
    let bundle = bundle();
    let mut problems = Vec::new();
    let mut paths = Vec::new();
    let mut loader_pages = 0;
    for (path, bytes) in files {
        paths.push(path);
        let file = path.rsplit('/').next().unwrap_or(path);
        let ext = file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        let ext = ext.as_deref().unwrap_or("");
        match (ext, &bundle) {
            ("wasm", Some(b)) => {
                if bytes != b.wasm {
                    problems.push(format!("{path} differs from the bundle's module"));
                }
                if bytes.len() > WASM_BUDGET {
                    problems.push(format!("{path} is over budget ({} bytes)", bytes.len()));
                }
            }
            ("js", Some(b)) if path == LOADER_PATH => {
                let got = sha256_hex(bytes);
                if got != b.loader_sha256 {
                    problems.push(format!(
                        "{path} is not the bundle's loader (sha256 {got}, want {})",
                        b.loader_sha256
                    ));
                }
                if bytes.len() > LOADER_BUDGET {
                    problems.push(format!("{path} is over budget ({} bytes)", bytes.len()));
                }
            }
            _ if SCRIPT_EXTENSIONS.contains(&ext) => {
                problems.push(format!("script file {path}"));
            }
            _ => {}
        }
        match ext {
            "html" | "htm" => {
                let (found, loaders) = audit_markup(path, bytes, true, bundle.is_some());
                problems.extend(found);
                loader_pages += usize::from(loaders > 0);
            }
            "svg" => problems.extend(audit_markup(path, bytes, false, false).0),
            _ => {}
        }
    }
    // A page that names the loader needs it, and its module, in the output.
    if loader_pages > 0 {
        let has = |p: &str| paths.contains(&p);
        if !has(LOADER_PATH) {
            problems.push(format!(
                "{SEARCH_PAGE} names {LOADER_PATH}, which is not in the output"
            ));
        } else if !has(WASM_PATH) {
            problems.push(format!(
                "{SEARCH_PAGE} names the loader, but {WASM_PATH} is not in the output"
            ));
        }
    }
    problems
}

/// Every problem in the directory `out`, walked without following links.
fn audit_site(out: &Path) -> Vec<String> {
    fn walk(
        dir: &Path,
        prefix: &str,
        files: &mut Vec<(String, Vec<u8>)>,
        problems: &mut Vec<String>,
    ) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {dir:?}: {e}"))
            .map(|e| e.unwrap_or_else(|e| panic!("read {dir:?}: {e}")))
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                walk(&entry.path(), &rel, files, problems);
            } else if kind.is_file() {
                let bytes =
                    std::fs::read(entry.path()).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                files.push((rel, bytes));
            } else {
                problems.push(format!("{rel} is not a regular file"));
            }
        }
    }
    let mut files = Vec::new();
    let mut problems = Vec::new();
    walk(out, "", &mut files, &mut problems);
    problems.extend(audit_files(
        files.iter().map(|(p, b)| (p.as_str(), b.as_slice())),
    ));
    problems
}

// ---------- helpers ----------

/// A directory under the system temp dir, removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rubrica-nojs-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The real path: a symbolic link in it (macOS's /var) is refused.
    Scratch(dir.canonicalize().unwrap())
}

/// `git` in `dir`, isolated from the user's and the system's config.
fn git_at(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git")
}

/// The repository root: the first ancestor of the crate's directory (itself included) that
/// holds both `docs/home.md` and `Cargo.toml`; `None` outside a checkout (a packaged crate).
fn repo_root() -> Option<PathBuf> {
    let start = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("docs/home.md").is_file() && d.join("Cargo.toml").is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Copies the directory `from` (files and directories only) to `to`.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The files an in-memory build writes, each once.
fn build_in_memory(files: Vec<SourceFile>) -> BTreeMap<String, Vec<u8>> {
    build_in_memory_with(files, false)
}

/// [`build_in_memory`]; with `explicit_bundle`, through `build_with_search_wasm`, which writes
/// the search bundle whatever `build` does.
fn build_in_memory_with(
    files: Vec<SourceFile>,
    explicit_bundle: bool,
) -> BTreeMap<String, Vec<u8>> {
    let sources: BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|f| (f.path.clone(), f.bytes.clone()))
        .collect();
    let site = Site::from_sources(files, &SiteConfig::default());
    let history = History {
        commit: Commit {
            id: "c".repeat(40),
            time: 1_792_981_257,
            offset: -7 * 3600,
            summary: "the commit built".into(),
        },
        last: BTreeMap::new(),
        recent: Vec::new(),
        feed_versions: Vec::new(),
        complete: true,
    };
    let cfg = BuildConfig {
        base_url: Some("https://wiki.example.org".into()),
        edit_url: Some("https://forge.example/edit/main/".into()),
        history_url: Some("https://forge.example/history/main/".into()),
        ..BuildConfig::default()
    };
    let mut out = BTreeMap::new();
    let mut put = |path: &str, bytes: &[u8]| {
        assert!(
            out.insert(path.to_owned(), bytes.to_vec()).is_none(),
            "{path} written twice"
        );
        Ok(())
    };
    let mut read = |page: &rubrica_site::Page| Ok(sources[&*page.file].clone());
    if explicit_bundle {
        #[cfg(feature = "search-wasm")]
        build::build_with_search_wasm(&site, &cfg, &history, &mut read, &mut put)
            .expect("the site builds");
        #[cfg(not(feature = "search-wasm"))]
        panic!("no bundle in this build");
    } else {
        build::build(&site, &cfg, &history, &mut read, &mut put).expect("the site builds");
    }
    out
}

fn audit_map(out: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    audit_files(out.iter().map(|(p, b)| (p.as_str(), b.as_slice())))
}

fn md(path: &str, body: &str) -> SourceFile {
    SourceFile::regular(path, body.as_bytes().to_vec())
}

/// A string as a YAML flow scalar (a JSON string is one).
fn yq(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

fn page(path: &str, title: &str, body: &str) -> SourceFile {
    md(path, &format!("---\ntitle: {}\n---\n{body}", yq(title)))
}

// ---------- the tests ----------

/// The auditor finds what it is for: a hand-made page with the policy meta, one script tag
/// and one handler attribute has exactly those two problems.
#[test]
fn the_audit_finds_a_planted_script() {
    let dir = scratch("planted");
    std::fs::write(
        dir.0.join("index.html"),
        "<!doctype html><html><head>\
         <meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'; connect-src 'none'\">\
         </head><body><script>alert(1)</script><a href=\"/x\" onclick=\"y()\">x</a></body></html>",
    )
    .unwrap();
    let problems = audit_site(&dir.0);
    assert!(
        problems.len() == 2,
        "problems.len() == 2 (got {}): {problems:?}",
        problems.len()
    );
    assert!(
        problems.iter().any(|p| p.contains("script")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.contains("onclick")),
        "{problems:?}"
    );
}

/// `build --subtree docs` of a commit of a copy of the project's `docs/`.
#[test]
fn the_docs_site_has_no_script() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate (docs/home.md and Cargo.toml): docs site skipped");
        return;
    };
    let site = scratch("docs");
    let repo = site.0.join("repo");
    copy_dir(&root.join("docs"), &repo.join("docs"));
    for args in [
        &["init", "-q"][..],
        &["add", "-A", "."][..],
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            "docs",
        ][..],
    ] {
        let out = git_at(&repo, args);
        assert!(out.status.success(), "git {args:?} failed");
    }
    let dest = site.0.join("out");
    let out = Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args(["build", "--subtree", "docs"])
        .arg(&repo)
        .arg(&dest)
        .output()
        .expect("run rubrica");
    assert!(
        out.status.success(),
        "build --subtree docs: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dest.join("home.html").is_file(), "home.html was not built");
    let problems = audit_site(&dest);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn a_hostile_site_has_no_script() {
    // The payloads of the sanitizer's own URL test, in every position a destination can be.
    let payloads = [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "JAVASCRIPT:alert(1)",
        "&#106;avascript:alert(1)",
        "&#x6A;avascript:alert(1)",
        "&#0000106;avascript:alert(1)",
        "javascript&colon;alert(1)",
        "javascript&#58;alert(1)",
        "java&#9;script:alert(1)",
        "java&#10;script:alert(1)",
        "java&Tab;script:alert(1)",
        "&#1;javascript:alert(1)",
        "&#x20;javascript:alert(1)",
        "&#14;javascript:alert(1)",
        "vbscript:msgbox(1)",
        "VBScript:msgbox(1)",
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "DATA:text/html,<script>alert(1)</script>",
        "data:image/svg+xml,<svg onload=alert(1)>",
        "data:image/png;base64,iVBORw0KGgo=",
        "JAVA\tSCRIPT:x",
        "data:text/html,x",
        "javascript:y",
    ];
    let handlers = [
        "<b onmouseover=alert(1)>x</b>\n",
        "<details ontoggle=alert(1) open>x</details>\n",
        "<svg><script>alert(1)</script></svg>\n",
        "<math><mi xlink:href=\"javascript:alert(1)\">x</mi></math>\n",
        "<iframe src=\"javascript:alert(1)\"></iframe>\n",
        "<p style=\"background:url(javascript:alert(1))\">x</p>\n",
        "[x](/u \"t\\\" onmouseover=\\\"alert(1)\")\n",
        "```js\" onmouseover=\"alert(1)\nx\n```\n",
        "<meta http-equiv=refresh content=\"0;url=javascript:alert(1)\">\n",
        "<base href=\"javascript:alert(1)//\">\n<object data=\"x\"></object><embed src=\"x\">\n",
        "# <img src=x onerror=y>\n",
        "```html\n<script>alert(1)</script>\n```\n",
        "```mermaid\ngraph TD; A-->B\nclick A href \"javascript:alert(1)\"\n%%{init: {\"securityLevel\": \"loose\"}}%%\n```\n",
    ];
    let mut files = vec![page("home.md", "Home", "# Home\n")];
    for (i, p) in payloads.iter().enumerate() {
        let body = [
            format!("[x]({p})\n"),
            format!("[x](<{p}>)\n"),
            format!("![x]({p})\n"),
            format!("[x][r{i}a]\n\n[r{i}a]: {p}\n"),
            format!("![x][r{i}b]\n\n[r{i}b]: <{p}>\n"),
            format!("[x]( {p} \"t\")\n"),
            format!("<{p}>\n"),
            format!("<a href=\"{p}\">x</a>\n"),
            format!("<img src=\"{p}\">\n"),
        ]
        .join("\n");
        files.push(page(&format!("payload/p{i}.md"), "Payload", &body));
    }
    files.push(page("handlers.md", "Handlers", &handlers.join("\n")));
    files.push(page("title-script.md", "<script>x</script>", "# T\n"));
    files.push(page(
        "title-break.md",
        "</title><script>alert(1)</script>",
        "# T\n",
    ));
    files.push(md(
        "meta.md",
        &format!(
            "---\ntitle: Meta\ndescription: {}\ntags: [\"onload=x\", \"\\\"><script>x</script>\"]\n---\n# Meta\n",
            yq("javascript:alert(1)")
        ),
    ));
    files.push(page("a.js.md", "A", "# A\n"));
    files.push(page("b.mjs.md", "B", "# B\n"));
    let out = build_in_memory(files);
    let pages = out.keys().filter(|p| p.ends_with(".html")).count();
    assert!(pages > payloads.len(), "{pages} pages");
    let problems = audit_map(&out);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// One page per CommonMark and GFM example: the raw HTML ones carry script tags, which the
/// sanitizer removes, and that is what this checks.
#[test]
fn the_spec_site_has_no_script() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate: spec site skipped");
        return;
    };
    let mut files = vec![page("home.md", "Home", "# Home\n")];
    let mut examples = 0;
    for (tag, name) in [("cm", "commonmark-0.31.2.json"), ("gfm", "gfm-0.29.json")] {
        let path = root.join("crates/rubrica-markdown/tests/spec").join(name);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for (i, ex) in json.as_array().unwrap().iter().enumerate() {
            let markdown = ex["markdown"].as_str().unwrap();
            files.push(page(
                &format!("spec/{tag}-{i:04}.md"),
                &format!("Example {tag} {i}"),
                markdown,
            ));
            examples += 1;
        }
    }
    assert!(examples >= 1_300, "{examples} examples");
    let out = build_in_memory(files);
    let pages = out.keys().filter(|p| p.ends_with(".html")).count();
    assert!(pages > examples, "{pages} pages for {examples} examples");
    let problems = audit_map(&out);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Audits the directory `RUBRICA_NO_JS_SITE` names (a release's built site); skips when unset.
#[test]
fn an_external_site_has_no_script() {
    let Some(dir) = std::env::var_os("RUBRICA_NO_JS_SITE") else {
        eprintln!("RUBRICA_NO_JS_SITE unset");
        return;
    };
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "RUBRICA_NO_JS_SITE is not a directory: {dir:?}"
    );
    let problems = audit_site(&dir);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// A redirect stub, as `build` writes it, is not a problem: it refreshes at once to a page of
/// the same site.
#[test]
fn a_site_with_a_redirect_has_no_script() {
    let out = build_in_memory(vec![
        page("home.md", "Home", "# Home\n"),
        page("new.md", "New", "# New\n\n## Part\n"),
        md(
            "old.md",
            "---\ntitle: Old\nredirect_to: /new#part\n---\nMoved.\n",
        ),
    ]);
    let stub = String::from_utf8(out["old.html"].clone()).unwrap();
    assert!(
        stub.contains("<meta http-equiv=\"refresh\" content=\"0; url=/new#part\">"),
        "{stub}"
    );
    let problems = audit_map(&out);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// A refresh is allowed only as `0; url=` and a same-site path; every other shape is a problem.
#[test]
fn the_audit_flags_a_refresh_that_leaves_the_site() {
    let csp = "<meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'; connect-src 'none'\">";
    for content in [
        "0; url=javascript:alert(1)",
        "0; url=//evil.example",
        "5; url=/x",
        "0; url=https://evil.example/",
        "0; url=http://evil.example/",
        "0; url=data:text/html,x",
        "0; url=mailto:a@b.example",
        "0; url=x",
        "0; url=",
        "0",
        "/x",
        "0;url=/x",
        "0; URL=/x",
        "0; url=/x y",
    ] {
        let html = format!(
            "<html><head>{csp}<meta http-equiv=\"refresh\" content=\"{content}\"></head></html>"
        );
        let problems = audit_files([("a.html", html.as_bytes())]);
        assert!(
            problems.len() == 1 && problems[0].contains("meta refresh"),
            "{content:?}: {problems:?}"
        );
    }
    for ok in ["0; url=/x", "0; url=/new#part", "0; url=/a/b?q=1#f"] {
        let html = format!(
            "<html><head>{csp}<meta http-equiv=\"refresh\" content=\"{ok}\"></head></html>"
        );
        let problems = audit_files([("a.html", html.as_bytes())]);
        assert!(problems.is_empty(), "{ok:?}: {problems:?}");
    }
}

// ---------- v2: the search page's loader and the per-page CSP ----------

/// The CSP of every page but the search page's: no script, no connection.
const CSP_NO_SCRIPT: &str = "default-src 'self'; script-src 'none'; connect-src 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'self'";

/// The CSP of the search page when it carries the loader: `'self'` for the module script's
/// own URL, `'wasm-unsafe-eval'` to instantiate the module, `connect-src 'self'` for the
/// index fetch.
#[cfg(feature = "search-wasm")]
const CSP_SEARCH_WASM: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'none'; form-action 'self'";

/// The one script tag any page may carry.
#[cfg(feature = "search-wasm")]
const LOADER_TAG: &str = "<script type=\"module\" src=\"/_assets/rubrica-search.js\"></script>";

/// The files of a site: path to bytes.
type Files = BTreeMap<String, Vec<u8>>;

/// A built site with pages at the top and in subdirectories, and a redirect stub.
fn v2_site(explicit_bundle: bool) -> Files {
    build_in_memory_with(
        vec![
            page("home.md", "Home", "# Home\n"),
            page("guide/intro.md", "Intro", "# Intro\n"),
            page("guide/deep/leaf.md", "Leaf", "# Leaf\n"),
            md(
                "old.md",
                "---\ntitle: Old\nredirect_to: /guide/intro\n---\nMoved.\n",
            ),
        ],
        explicit_bundle,
    )
}

/// The `content` of a page's Content-Security-Policy meta ("" when it has none).
fn csp_of(html: &str) -> String {
    let marker = "http-equiv=\"Content-Security-Policy\" content=\"";
    html.find(marker)
        .map(|i| &html[i + marker.len()..])
        .and_then(|rest| rest.find('"').map(|j| rest[..j].to_owned()))
        .unwrap_or_default()
}

fn utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(feature = "search-wasm")]
#[test]
fn only_the_search_page_carries_the_loader_and_the_wasm_csp() {
    let out = v2_site(false);
    let search = utf8(&out["_search.html"]);
    let csp = csp_of(&search);
    assert!(
        csp.contains("'wasm-unsafe-eval'"),
        "CSP on _search.html lacks 'wasm-unsafe-eval': {csp}"
    );
    assert_eq!(csp, CSP_SEARCH_WASM, "CSP on _search.html");
    let mut tagged = Vec::new();
    for (path, bytes) in out.iter().filter(|(p, _)| p.ends_with(".html")) {
        let html = utf8(bytes);
        let has_tag = html.contains(LOADER_TAG);
        let scripts = html.to_ascii_lowercase().matches("<script").count();
        assert_eq!(scripts, usize::from(has_tag), "script tags in {path}");
        if has_tag {
            tagged.push(path.as_str());
        } else {
            assert!(
                !csp_of(&html).contains("wasm-unsafe-eval"),
                "{path} has the wasm CSP and no loader"
            );
        }
    }
    assert_eq!(tagged, ["_search.html"], "pages with the loader");
    let problems = audit_map(&out);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_other_page_has_script_src_none_and_connect_src_none() {
    let out = v2_site(false);
    let mut others = 0;
    let mut nested = 0;
    for (path, bytes) in out.iter().filter(|(p, _)| p.ends_with(".html")) {
        if cfg!(feature = "search-wasm") && path == "_search.html" {
            continue;
        }
        others += 1;
        nested += usize::from(path.contains('/'));
        let csp = csp_of(&utf8(bytes));
        assert!(
            csp.contains("script-src 'none'"),
            "CSP on {path} lacks script-src 'none': {csp}"
        );
        assert!(
            csp.contains("connect-src 'none'"),
            "CSP on {path} lacks connect-src 'none': {csp}"
        );
        assert_eq!(csp, CSP_NO_SCRIPT, "CSP on {path}");
    }
    assert!(others >= 6, "{others} pages checked");
    assert!(nested >= 2, "{nested} nested pages checked");
}

/// The bundle's files in a built site are the bundle crate's, byte for byte.
#[cfg(feature = "search-wasm")]
#[test]
fn the_loader_on_disk_is_the_bundles_loader_byte_for_byte() {
    use sha2::{Digest, Sha256};
    let out = v2_site(true);
    let loader = &out["_assets/rubrica-search.js"];
    assert_eq!(loader.as_slice(), rubrica_wasm_bundle::LOADER.as_bytes());
    assert_eq!(
        Sha256::digest(loader),
        Sha256::digest(rubrica_wasm_bundle::LOADER.as_bytes())
    );
    assert_eq!(
        out["_assets/rubrica-search_bg.wasm"].as_slice(),
        rubrica_wasm_bundle::WASM
    );
}

#[cfg(feature = "search-wasm")]
#[test]
fn the_wasm_and_loader_are_under_budget_in_the_output() {
    let out = v2_site(true);
    let wasm = out["_assets/rubrica-search_bg.wasm"].len();
    let loader = out["_assets/rubrica-search.js"].len();
    assert!((1..=1_572_864).contains(&wasm), "the .wasm is {wasm} bytes");
    assert!(
        (1..=32_768).contains(&loader),
        "the loader is {loader} bytes"
    );
}

/// A page as the auditor reads it: the policy meta and a body.
#[cfg(feature = "search-wasm")]
fn html_page(csp: &str, body: &str) -> Vec<u8> {
    format!(
        "<!doctype html><html><head><meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\"></head><body>{body}</body></html>"
    )
    .into_bytes()
}

/// A clean v2 site: two pages with no script, the search page with the loader, and the
/// bundle's two files.
#[cfg(feature = "search-wasm")]
fn good_files() -> Files {
    let mut m = BTreeMap::new();
    m.insert("index.html".into(), html_page(CSP_NO_SCRIPT, "<p>home</p>"));
    m.insert("guide/a.html".into(), html_page(CSP_NO_SCRIPT, "<p>a</p>"));
    m.insert(
        "_search.html".into(),
        html_page(CSP_SEARCH_WASM, &format!("<main>search</main>{LOADER_TAG}")),
    );
    m.insert(
        "_assets/rubrica-search.js".into(),
        rubrica_wasm_bundle::LOADER.as_bytes().to_vec(),
    );
    m.insert(
        "_assets/rubrica-search_bg.wasm".into(),
        rubrica_wasm_bundle::WASM.to_vec(),
    );
    m
}

/// The auditor's problems for a map of files.
#[cfg(feature = "search-wasm")]
fn problems_of(files: &Files) -> Vec<String> {
    audit_map(files)
}

/// The auditor catches what v2 forbids: a second script tag, a changed loader or module, the
/// loader anywhere else, the wrong CSP for a page's tag, inline content, an extra attribute, a
/// loader that is not there, a file over budget. Each case starts from a clean site, which
/// itself has no problem.
#[cfg(feature = "search-wasm")]
#[test]
fn a_planted_second_script_tag_or_a_changed_loader_is_caught() {
    let good = good_files();
    let clean = problems_of(&good);
    assert!(clean.is_empty(), "a clean v2 site has problems: {clean:#?}");

    let with = |f: &dyn Fn(&mut Files)| {
        let mut m = good.clone();
        f(&mut m);
        m
    };
    let search_with = |csp: &str, body: &str| html_page(csp, body);
    let cases: Vec<(&str, Files, &str)> = vec![
        (
            "a second script tag",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(
                        CSP_SEARCH_WASM,
                        &format!("{LOADER_TAG}<script src=\"/x.js\"></script>"),
                    ),
                );
            }),
            "script tag in _search.html",
        ),
        (
            "the loader tag twice",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(CSP_SEARCH_WASM, &format!("{LOADER_TAG}{LOADER_TAG}")),
                );
            }),
            "2 loader script tags in _search.html",
        ),
        (
            "a changed loader",
            with(&|m| {
                m.get_mut("_assets/rubrica-search.js").unwrap().push(b'\n');
            }),
            "_assets/rubrica-search.js is not the bundle's loader (sha256",
        ),
        (
            "a changed module",
            with(&|m| {
                m.get_mut("_assets/rubrica-search_bg.wasm").unwrap()[0] ^= 1;
            }),
            "_assets/rubrica-search_bg.wasm differs from the bundle's",
        ),
        (
            "another wasm file",
            with(&|m| {
                m.insert("x/other.wasm".into(), b"\0asm".to_vec());
            }),
            "x/other.wasm differs from the bundle's",
        ),
        (
            "the loader on another page",
            with(&|m| {
                m.insert(
                    "index.html".into(),
                    html_page(CSP_SEARCH_WASM, &format!("<p>x</p>{LOADER_TAG}")),
                );
            }),
            "the loader script tag is on index.html; only _search.html may carry it",
        ),
        (
            "the loader tag under the no-script CSP",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(CSP_NO_SCRIPT, LOADER_TAG),
                );
            }),
            "the CSP meta in _search.html lacks script-src 'self' 'wasm-unsafe-eval'",
        ),
        (
            "the wasm CSP without the loader tag",
            with(&|m| {
                m.insert("index.html".into(), html_page(CSP_SEARCH_WASM, "<p>x</p>"));
            }),
            "the CSP meta in index.html lacks script-src 'none'",
        ),
        (
            "no connect-src on a page without the loader",
            with(&|m| {
                m.insert(
                    "index.html".into(),
                    html_page("default-src 'self'; script-src 'none'", "<p>x</p>"),
                );
            }),
            "the CSP meta in index.html lacks connect-src 'none'",
        ),
        (
            "a connect-src that is not 'self' on the search page",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(
                        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src *",
                        LOADER_TAG,
                    ),
                );
            }),
            "the CSP meta in _search.html lacks connect-src 'self'",
        ),
        (
            "inline content in the loader tag",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(
                        CSP_SEARCH_WASM,
                        "<script type=\"module\" src=\"/_assets/rubrica-search.js\">alert(1)</script>",
                    ),
                );
            }),
            "content inside the loader script tag in _search.html",
        ),
        (
            "an extra attribute on the loader tag",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(
                        CSP_SEARCH_WASM,
                        "<script type=\"module\" src=\"/_assets/rubrica-search.js\" async></script>",
                    ),
                );
            }),
            "script tag in _search.html",
        ),
        (
            "a classic script for the loader",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(
                        CSP_SEARCH_WASM,
                        "<script src=\"/_assets/rubrica-search.js\"></script>",
                    ),
                );
            }),
            "script tag in _search.html",
        ),
        (
            "another source for the module script",
            with(&|m| {
                m.insert(
                    "_search.html".into(),
                    search_with(
                        CSP_SEARCH_WASM,
                        "<script type=\"module\" src=\"/_assets/other.js\"></script>",
                    ),
                );
            }),
            "script tag in _search.html",
        ),
        (
            "a loader script in an svg",
            with(&|m| {
                m.insert(
                    "_assets/x.svg".into(),
                    format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{LOADER_TAG}</svg>")
                        .into_bytes(),
                );
            }),
            "script tag in _assets/x.svg",
        ),
        (
            "a loader tag with no loader file",
            with(&|m| {
                m.remove("_assets/rubrica-search.js");
            }),
            "_search.html names _assets/rubrica-search.js, which is not in the output",
        ),
        (
            "a loader tag with no module",
            with(&|m| {
                m.remove("_assets/rubrica-search_bg.wasm");
            }),
            "_search.html names the loader, but _assets/rubrica-search_bg.wasm is not in the output",
        ),
        (
            "a module over budget",
            with(&|m| {
                m.insert(
                    "_assets/rubrica-search_bg.wasm".into(),
                    vec![0; 1_572_865],
                );
            }),
            "_assets/rubrica-search_bg.wasm is over budget",
        ),
        (
            "a loader over budget",
            with(&|m| {
                m.insert("_assets/rubrica-search.js".into(), vec![b' '; 32_769]);
            }),
            "_assets/rubrica-search.js is over budget",
        ),
        (
            "a module script file",
            with(&|m| {
                m.insert("_assets/x.mjs".into(), b"x".to_vec());
            }),
            "script file _assets/x.mjs",
        ),
        (
            "a second js file",
            with(&|m| {
                m.insert("_assets/other.js".into(), b"x".to_vec());
            }),
            "script file _assets/other.js",
        ),
    ];
    for (name, files, needle) in cases {
        let problems = problems_of(&files);
        assert!(
            problems.iter().any(|p| p.contains(needle)),
            "{name}: no problem holds {needle:?}: {problems:#?}"
        );
    }
}
