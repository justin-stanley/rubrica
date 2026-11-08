#!/usr/bin/env node
// crates/rubrica-site/tests/fixtures/wikijs-path-differential.json: Wiki.js 2.5.314's own `parsePath`
// and `getPagePath` on generated paths, for rubrica-site's ports
// (`source::wikijs_parse_path`, `source::wikijs_page_path`; the test is
// tests/paths.rs `parse_path_and_get_page_path_match_wikijs_on_every_differential_row`).
//
// The functions are not copied out: tools/vendor/wikijs-2.5.314-page.js is
// the container's server/helpers/page.js byte for byte (sha256 checked
// below), loaded with WIKI.config.lang.code 'en' and lodash 4.17.21, whose
// lodash.js is checked against the container's sha256 too. Get it with
//
//   npm pack lodash@4.17.21 && tar -xzf lodash-4.17.21.tgz
//   LODASH=$PWD/package/lodash.js env -u NODE_OPTIONS node tools/gen-wikijs-path-differential.js > crates/rubrica-site/tests/fixtures/wikijs-path-differential.json
//
// Each port's domain (its docs say so) is what is generated: raw paths with
// no `%` for parsePath (qs.unescape is not ported), and `.md` files whose
// stem is a page path for getPagePath.
'use strict';
const crypto = require('crypto');
const fs = require('fs');
const Module = require('module');
const path = require('path');

const PAGE_JS_SHA256 = '26e161f799d218e179b67f2c775461a8d6560f7f48e05f21472c9bcf3133e9c9';
const LODASH_SHA256 = '4c04561befdf653aef017a42ac5addf68ea943cdfca6bdee5ce04e04e8139f54';
const sha256 = (b) => crypto.createHash('sha256').update(b).digest('hex');

const pageFile = path.join(__dirname, 'vendor/wikijs-2.5.314-page.js');
const pageSrc = fs.readFileSync(pageFile);
if (sha256(pageSrc) !== PAGE_JS_SHA256) throw new Error(`${pageFile} is not Wiki.js 2.5.314's page.js`);
const lodashFile = process.env.LODASH;
if (!lodashFile) throw new Error('set LODASH to lodash 4.17.21\'s lodash.js');
if (sha256(fs.readFileSync(lodashFile)) !== LODASH_SHA256) throw new Error(`${lodashFile} is not lodash 4.17.21`);

global.WIKI = { config: { lang: { code: 'en' } }, data: { reservedPaths: [] } };
const lodash = require(lodashFile);
const m = new Module(pageFile);
m.filename = pageFile;
m.paths = [];
m.require = (id) => (id === 'lodash' ? lodash : require(id));
m._compile(pageSrc.toString('utf8'), pageFile);
const { parsePath, getPagePath } = m.exports;

// xorshift, seeded: the same rows every run.
let seed = 0x9e3779b9;
const rnd = (n) => {
  seed ^= seed << 13; seed >>>= 0;
  seed ^= seed >>> 17;
  seed ^= seed << 5; seed >>>= 0;
  return seed % n;
};
const pick = (a) => a[rnd(a.length)];

const RAW = ['/', '/', '/', '//', 'a', 'b', 'abc', 'xyz', 'en', 'EN', 'fr', 'ab-cd', 'Ab-CD', 'abc-de',
  't', 'x', '1', '9', '-', '_', 'é', 'é', '\u{1D49C}', '😀', '.', '..', '...', 'a.b', 'v2.1', '.md',
  ' ', '  ', '\t', ' ', '　', '﻿', '\u0085', ' ', '\n', '\x01', '\x7f', '\x85',
  '"', '|', '<', '>', ':', '*', '?', '\\', 'home', 'login', 'index', '_x', 'a b', 'Z'];
const parse = [];
const seen = new Set();
const addRaw = (raw) => {
  if (seen.has(raw) || raw.includes('%')) return;
  seen.add(raw);
  let out;
  try {
    const p = parsePath(raw);
    out = [p.locale, p.path, p.explicitLocale];
  } catch (e) {
    if (!(e instanceof TypeError)) throw e;
    out = null; // pathParts[0] is undefined: it throws.
  }
  parse.push([raw, out]);
};
for (const r of ['', '/', '/home', '/a/b', '/t/x', '/1/ab/c', '/./', '/x', '/abc/.../d', '/a..b/c']) addRaw(r);
while (parse.length < 24000) {
  let s = rnd(4) ? '/' : '';
  for (let i = 1 + rnd(7); i > 0; i--) s += pick(RAW);
  addRaw(s);
}

// Stems of page paths: no control character, `%`, `\`, `/`-edge or
// `?#"|<>:*`; dots, locale-shaped folders, one-letter folders.
const SEG = ['a', 'abc', 'xyz', 'en', 'fr', 'ab-cd', 'ab', 'v2.1', 'a.b', 'a..b', 'x.y.z', 'é', '\u{1D49C}',
  '1', 't', 'home', 'index', 'a b', 'A-B', 'readme', 'notes.v2', 'z'];
const files = [];
const seenF = new Set();
while (files.length < 12000) {
  const segs = [];
  for (let i = 1 + rnd(4); i > 0; i--) segs.push(pick(SEG));
  const f = segs.join('/') + '.md';
  if (seenF.has(f)) continue;
  seenF.add(f);
  const p = getPagePath(f);
  files.push([f, p.locale, p.path]);
}

process.stdout.write(JSON.stringify({
  generator: 'tools/gen-wikijs-path-differential.js',
  node: process.version,
  page_js_sha256: PAGE_JS_SHA256,
  lodash_sha256: LODASH_SHA256,
  parse_path: parse,
  get_page_path: files,
}) + '\n');
