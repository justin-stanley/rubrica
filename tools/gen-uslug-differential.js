#!/usr/bin/env node
// Generates crates/rubrica-render/tests/fixtures/uslug-differential.json: inputs run through the REAL
// uslug@1.0.4 (with unorm@1.6.0, the versions Wiki.js 2.5.314's yarn.lock
// pins), then through Wiki.js's own heading-id step (server/modules/rendering/
// html-core/renderer.js at v2.5.314, lines ~203-232: the `h-` guard and the
// `-N` dedupe). crates/rubrica-render/tests/uslug_differential.rs must
// reproduce every row.
//
// Run by hand from a scratch dir (the JS never enters the repo or the binary):
//
//   mkdir /tmp/uslug && cd /tmp/uslug && npm init -y >/dev/null
//   npm install --no-save uslug@1.0.4 unorm@1.6.0
//   node <rubrica>/tools/gen-uslug-differential.js <rubrica>
//
// Deterministic: the random parts use a fixed-seed PRNG.
'use strict';
const fs = require('fs');
const path = require('path');

const repo = process.argv[2];
if (!repo) throw new Error('usage: gen-uslug-differential.js <rubrica checkout>');
const from = (m) => require(require.resolve(m, { paths: [process.cwd()] }));
for (const [m, v] of [['uslug', '1.0.4'], ['unorm', '1.6.0']]) {
  const got = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'node_modules', m, 'package.json'), 'utf8')).version;
  if (got !== v) throw new Error(`${m} is ${got}, want ${v}`);
}
// uslug lowercases with JS toLowerCase, whose Unicode version is the runtime's
// ICU: under Unicode 16 "ΑΣʕΒ" gives "ασʕβ", under 17 "αςʕβ" (U+0295 stopped
// being Cased). Wiki.js runs Node 24.15 / ICU 78 (Unicode 17); so must this.
if (process.versions.unicode !== '17.0') throw new Error(`Node has Unicode ${process.versions.unicode}, want 17.0 (ICU 78)`);
const uslug = from('uslug');
const unorm = from('unorm');
const T = Object.fromEntries(['L', 'N', 'M', 'Z'].map((n) => [n, from(`uslug/lib/${n}`)[n]]));

// Wiki.js 2.5.314 renderer.js, the slug part, verbatim in effect.
function headingIds(texts) {
  const headers = [];
  return texts.map((text) => {
    let headerSlug = uslug(text);
    if (headerSlug.match(/^\d/)) headerSlug = `h-${headerSlug}`;
    if (headers.indexOf(headerSlug) >= 0) {
      let hIdx = 1;
      while (headers.indexOf(`${headerSlug}-${hIdx}`) >= 0) hIdx++;
      headerSlug = `${headerSlug}-${hIdx}`;
    }
    headers.push(headerSlug);
    return headerSlug;
  });
}

const isSurrogate = (c) => c >= 0xd800 && c <= 0xdfff;
const ch = (c) => String.fromCodePoint(c);
const inputs = new Map(); // input -> group
const add = (group, s) => { if (!inputs.has(s)) inputs.set(s, group); };

// 1. The reviewer's realistic cases, and the plan's unit-test cases.
for (const s of [
  'Docker™ setup', 'Latency in µs', '№ 5 plan', 'Temp ℃', 'x²', '½ cup', '① Intro', 'ﬁle sizes',
  'ＡＢＣ test', 'ǅemal', 'Café', 'Café', '日本語、テスト', '𠮷野家', '𝐀𝐁𝐂 bold', 'ᲐᲑ Georgian',
  'a\tb', 'Foo\nbar', 'x﻿y', 'a b', 'a\u0085b', 'a᠎b', 'a​b', 'a　b', 'a、b',
  'a。b', 'a！b', 'a＂b', 'a/b', 'Q&A', 'v2.1', 'a & b', 'Capacity & memory',
  'Memory sizing — why it OOMs', '4. NAS config backup', '_x', '~x', 'Ω', '-x', '٣ x',
  '⚠️ Open item', '✅ Done', 'FREE_POOL_SIZE sizing', 'Ünïcodé Ω', 'İstanbul', 'İ',
  'ΟΔΟΣ ΟΔΟΣ', 'Σ', 'ΑΣ1', 'ΑΣ-Β', 'ΑΣ.Β', "ΑΣ'Β", 'ẞ', 'ǅ', 'K', 'Ω', 'ﬀ', 'ᾈ',
  'a -- b', 'a--b', 'a - - b', '-a-', ' - a', '— !', '', '   ', 'What lives there', 'x 1',
  '👩‍💻 Dev', '🏳️‍🌈 flag', '1️⃣ step', '#️⃣ hash', 'é́', 'Ａ１', 'ｶﾀｶﾅ', 'ｶﾞ',
  '각', '각', 'ﾟ', 'Ⅻ chapter', 'ⓐⓑ', '㍿', '㎏', 'ℌ', 'ℵ', 'İ̇',
  'Open', 'Implementation', 'style', 'constructor', '__proto__', 'onclick',
  // Lowercasing that changed with Unicode 17 (U+0295 no longer Cased).
  'ΑΣʕΒ',
  // Combining marks newer than unorm's data (a starter to unorm), between marks
  // the crate would compose or reorder across.
  'e\u1DFA\u0301', 'Cafe\u1ABF\u0301', 'q\u0301\u1DFA\u0316', 'e\u{1E944}\u0301',
  // Canonical compositions newer than unorm's data with a BMP second component
  // (Vithkuqi): the crate composes to a non-BMP letter uslug drops, unorm keeps
  // the U+0307.
  'x\u{105D2}\u0307', '\u{105D2}\u0307', '\u00CB\u{105DA}\u0307 y',
]) add('cases', s);

// 2. Every whitespace and separator-ish code point, between letters.
const ws = [0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x180e, 0x2000, 0x2001, 0x2002,
  0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x200b, 0x200c, 0x200d, 0x2028,
  0x2029, 0x202f, 0x205f, 0x2060, 0x3000, 0x3001, 0x3002, 0xfeff, 0xff01, 0xff02];
for (const c of ws) { add('whitespace', `a${ch(c)}b`); add('whitespace', `${ch(c)}a${ch(c)}`); }

// 3. Boundaries: uslug's own CJK and Hangul ranges first (the category edges
// below would otherwise claim most of these rows), then each range edge of
// uslug's tables, and one past it.
for (const [lo, hi] of [[0x4e00, 0x9fff], [0xac00, 0xd7a3]]) {
  for (const c of [lo - 1, lo, lo + 1, hi - 1, hi, hi + 1]) add('boundary-range', `a${ch(c)}b`);
}
for (const n of ['L', 'N', 'M', 'Z']) {
  const set = new Set(T[n]);
  for (const c of T[n]) {
    if (c > 0xffff) continue;
    const start = !set.has(c - 1), end = !set.has(c + 1);
    // A range's first and last member, and the code point just outside it.
    const picks = [...(start ? [c - 1, c] : []), ...(end ? [c, c + 1] : [])];
    for (const d of picks) {
      if (d < 0x20 || d > 0xffff || isSurrogate(d)) continue;
      add(`boundary-${n}`, `a${ch(d)}b`);
    }
  }
}

// 4. Every code point unorm's NFKC changes (BMP and beyond), alone and between letters.
for (let c = 0x20; c <= 0x10ffff; c++) {
  if (isSurrogate(c)) continue;
  const s = ch(c);
  if (unorm.nfkc(s) !== s) { add('nfkc', s); add('nfkc', `a${s}b`); }
}

// 4b. Every code point where unorm's NFKC differs from ICU's (Unicode-version drift).
for (let c = 0x20; c <= 0x10ffff; c++) {
  if (isSurrogate(c)) continue;
  const s = ch(c);
  if (unorm.nfkc(s) !== s.normalize('NFKC')) { add('nfkc-drift', s); add('nfkc-drift', `a${s}b`); add('nfkc-drift', `${s}\u0301 x`); }
}

// 5. NFD/NFC pairs and composition across a kept/dropped boundary.
for (const s of ['é', 'Å', 'ñ', 'ṩ', 'ǖ', 'ệ', '가', 'がぎ', 'ﾊﾟ']) {
  add('nfd-nfc', unorm.nfd(s)); add('nfd-nfc', unorm.nfc(s)); add('nfd-nfc', `${unorm.nfd(s)} X`);
}
add('nfd-nfc', 'á!́'); add('nfd-nfc', 'q̣̇'); add('nfd-nfc', '́x');

// 5b. Every combining mark that ICU (Unicode 17) or the unicode-normalization
// crate (Unicode 18, the version Cargo.lock pins) gives a nonzero canonical
// combining class and unorm does not (it has no data for it, so a starter):
// between a base and a mark that composes with it, and between two marks of
// other classes. The crate's classes are read from its own table (each entry is
// code point << 8 | class), since Node's ICU is a version behind; the unit test
// `every_mark_newer_than_unorm_has_fixture_rows` (slug.rs) checks none is missed.
const crateMarks = (() => {
  const meta = JSON.parse(require('child_process').execFileSync('cargo',
    ['metadata', '--format-version', '1', '--locked', '--manifest-path', path.join(repo, 'Cargo.toml')],
    { encoding: 'utf8', maxBuffer: 1 << 28 }));
  const pkgs = meta.packages.filter((p) => p.name === 'unicode-normalization');
  if (pkgs.length !== 1) {
    throw new Error(`the dependency graph has ${pkgs.length} copies of unicode-normalization (${pkgs.map((p) => p.version).join(', ') || 'none'}), want exactly one, 0.1.26`);
  }
  if (pkgs[0].version !== '0.1.26') throw new Error(`unicode-normalization is ${pkgs[0].version}, want 0.1.26`);
  const tables = fs.readFileSync(path.join(path.dirname(pkgs[0].manifest_path), 'src/tables.rs'), 'utf8');
  if (!/UNICODE_VERSION: \(u8, u8, u8\) = \(18, 0, 0\)/.test(tables)) throw new Error('the crate is not Unicode 18');
  const kv = tables.match(/CANONICAL_COMBINING_CLASS_KV: &\[u32\] = &\[([^\]]*)\]/);
  if (!kv) throw new Error('no CANONICAL_COMBINING_CLASS_KV in the crate');
  const marks = new Map(kv[1].match(/0x[0-9A-Fa-f]+/g).map((h) => [parseInt(h, 16) >>> 8, parseInt(h, 16) & 0xff]));
  if (marks.size < 900 || ![...marks.values()].every((k) => k > 0)) throw new Error('misread the crate CCC table');
  return marks;
})();
const nonStarter = (nfd, s) => nfd('a\u0345' + s) !== 'a\u0345' + nfd(s) || nfd('a' + s + '\u0334') !== 'a' + nfd(s) + '\u0334';
for (let c = 0x20; c <= 0x10ffff; c++) {
  if (isSurrogate(c)) continue;
  const s = ch(c);
  const icu = s.normalize('NFD') === s && nonStarter((t) => t.normalize('NFD'), s);
  if (icu && !crateMarks.has(c)) throw new Error(`ICU gives ${c.toString(16)} a class and the crate does not`);
  if (unorm.nfd(s) !== s || s.normalize('NFD') !== s) continue;
  if ((icu || crateMarks.has(c)) && !nonStarter(unorm.nfd, s)) {
    add('newer-marks', `e${s}\u0301`); add('newer-marks', `e${s}\u0323`); add('newer-marks', `q\u0301${s}\u0316 x`);
  }
}

// 5c. Every primary composite (its own NFC, decomposing canonically) that ICU
// forms and unorm does not, decomposed, in a few contexts: the crate composes
// these where unorm keeps the parts. (The crate, Unicode 18, adds none beyond
// ICU's Unicode 17; the unit test that walks the crate's composites checks.)
for (let c = 0x20; c <= 0x10ffff; c++) {
  if (isSurrogate(c)) continue;
  const s = ch(c);
  const d = s.normalize('NFD');
  if ([...d].length < 2 || d.normalize('NFC') !== s || unorm.nfc(d) === s) continue;
  for (const t of [d, `x${d}`, `${d} y`, `\u00CB${d} y`, `${d}\u0301`, `${s}\u0307`]) add('newer-composites', t);
}

// 6. Non-BMP letters, digits, emoji; digits from other scripts.
for (const c of [0x10000, 0x10400, 0x10428, 0x1d400, 0x1d7ce, 0x1f600, 0x1f4a9, 0x1f1fa, 0x20000,
  0x2a6d6, 0x2f800, 0xe0041, 0x104a0, 0x1e900, 0x1e922, 0x16e40]) {
  add('non-bmp', `a${ch(c)}b`); add('non-bmp', `${ch(c)} x`);
}
for (const z of [0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xe50, 0xf20, 0x1040, 0x17e0,
  0x1810, 0xff10, 0x1d7ce, 0x104a0]) {
  for (let k = 0; k < 10; k++) add('digits', `${ch(z + k)}. item`);
}

// 7. Random strings over a pool of the interesting characters (fixed seed), and long ones.
// mulberry32
let seed = 0x5eed1234;
const rand = () => {
  seed = (seed + 0x6d2b79f5) >>> 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32;
};
const pool = [...'aZz09_-~ !./&—–·•’\'"()[]{}#<>:;,?*+=|%$@^`ÀéİıΣσςßǅ', '\t', '\n', ' ',
  '﻿', '　', '、', '​', '‍', '️', '́', '̈', '①', '½', '™',
  'µ', 'ﬁ', 'Ａ', 'ｶ', '日', '한', '𝐀', '😀', '𠮷', 'Ა', 'ꟲ', '℃', '№', '²'];
const pick = () => pool[Math.floor(rand() * pool.length)];
for (let i = 0; i < 1500; i++) {
  const n = 1 + Math.floor(rand() * 24);
  let s = ''; for (let j = 0; j < n; j++) s += pick();
  add('random', s);
}
for (let i = 0; i < 20; i++) {
  let s = ''; for (let j = 0; j < 400 + Math.floor(rand() * 1600); j++) s += pick();
  add('long', s);
}

const rows = [...inputs].map(([input, group]) => ({ group, input, slug: uslug(input), id: headingIds([input])[0] }));

// Dedupe sequences, each one page.
const pages = [
  ['What lives there', 'What lives there', 'What lives there'],
  ['x', 'x 1', 'x'], ['x', 'x', 'x', 'x 2'], ['—', '?!', '…'],
  ['4 x', '4 x', 'h-4-x', 'h 4 x'], ['Open', 'Open', 'open-1'], ['ﬁle', 'file', 'FILE'],
  ['½', '1 2', '12', 'h-12'], ['Ａ', 'a', 'A'], ['x\ty', 'xy', 'x y'],
].map((texts) => ({ texts, ids: headingIds(texts) }));

// 8. Exhaustive, as a digest: every non-surrogate code point in five contexts,
// one line each,
// "<hex>\t<uslug('a'+c+'b')>\t<uslug(c+'\u0301 X')>\t<uslug('e'+c+'\u0301')>\t<uslug('e'+c+'\u0323')>\t<uslug('x'+c+'\u0307')>\n".
// The third and fourth put c between a base and a mark that composes with it
// (U+0301 is class 230, U+0323 class 220), so a c that blocks or reorders
// differently in unorm than in the crate shows up; the fifth puts U+0307 after
// c, so a c that composes with a following mark in the crate and not in unorm
// (U+105D2, U+105DA) shows up.
const hash = require('crypto').createHash('sha256');
let exhaustiveCount = 0;
for (let c = 0; c <= 0x10ffff; c++) {
  if (isSurrogate(c)) continue;
  const s = ch(c);
  hash.update(`${c.toString(16)}\t${uslug(`a${s}b`)}\t${uslug(`${s}\u0301 X`)}\t${uslug(`e${s}\u0301`)}\t${uslug(`e${s}\u0323`)}\t${uslug(`x${s}\u0307`)}\n`);
  exhaustiveCount++;
}
const exhaustive = { code_points: exhaustiveCount, sha256: hash.digest('hex') };

const out = {
  generated_by: 'tools/gen-uslug-differential.js',
  uslug: '1.0.4', unorm: '1.6.0', node: process.version,
  exhaustive, row_count: rows.length, rows, pages,
};
const file = path.join(repo, 'crates/rubrica-render/tests/fixtures/uslug-differential.json');
fs.writeFileSync(file, JSON.stringify(out, null, 0).replace(/\},\{"group"/g, '},\n{"group"') + '\n');
const byGroup = {}; for (const r of rows) byGroup[r.group] = (byGroup[r.group] || 0) + 1;
console.log(`wrote ${file}: ${rows.length} rows, ${pages.length} pages`, byGroup);
