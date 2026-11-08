#!/usr/bin/env node
// Generates crates/rubrica-render/src/uslug_tables.rs: the BMP entries of
// uslug@1.0.4's own category tables (lib/L.js, N.js, M.js, Z.js), as sorted
// inclusive ranges (uslug walks UTF-16 code units, so its non-BMP entries can
// never match and are left out); UNORM_NFKC: every code point that
// unorm@1.6.0's NFKC changes on its own, with unorm's result; and UNORM_CCC:
// every code point to which unorm gives a nonzero canonical combining class,
// with that class. unorm's Unicode data is older than the crate's (it knows
// Unicode 7's combining marks, e.g. U+1AB0, but not later ones), and its
// Hangul range is one too long (it decomposes U+D7A4); its version is not
// stated, so both tables are determined empirically, by running unorm. Also
// UNORM_NO_COMPOSE: the first component of every canonical composite ICU forms
// and unorm does not (Unicode 13-16 scripts; two of them, U+105D2 and U+105DA,
// compose with the BMP U+0307), and, for the tests only, UNORM_COMPOSITES,
// UNORM_SPLIT (the code points the slugger splits at, predicted from the crate's
// view) and UNORM_NFKD_SHA256 (a digest of unorm's NFKD of every other one).
// Rubrica normalises with the unicode-normalization crate (Unicode 18) and uses
// these tables to find the code points where the crate's newer data disagrees
// with unorm.
//
// Not a build step: run by hand when the target changes, from a scratch dir
// (the JS is never vendored into the repo or the binary):
//
//   mkdir /tmp/uslug && cd /tmp/uslug && npm init -y >/dev/null
//   npm install --no-save uslug@1.0.4 unorm@1.6.0
//   node <rubrica>/tools/gen-uslug-tables.js <rubrica>
//
// Then `cargo fmt` is not needed: the output is already formatted.
'use strict';
const fs = require('fs');
const path = require('path');

const repo = process.argv[2];
if (!repo) throw new Error('usage: gen-uslug-tables.js <rubrica checkout>');
const from = (m) => require(require.resolve(m, { paths: [process.cwd()] }));
const pkg = from('uslug/package.json');
if (pkg.version !== '1.0.4') throw new Error(`uslug is ${pkg.version}, want 1.0.4`);
const license = fs.readFileSync(require.resolve('uslug/LICENSE', { paths: [process.cwd()] }), 'utf8').trim();

function ranges(codes) {
  const bmp = codes.filter((c) => c <= 0xffff);
  const out = [];
  for (const c of bmp) {
    const last = out[out.length - 1];
    if (last && c === last[1] + 1) last[1] = c;
    else if (last && c <= last[1]) throw new Error('unsorted table');
    else out.push([c, c]);
  }
  return { bmp: bmp.length, all: codes.length, out };
}

const hex = (c) => '0x' + c.toString(16).toUpperCase().padStart(4, '0');
let body = '';
const sets = {};
for (const name of ['L', 'N', 'M', 'Z']) {
  const codes = from(`uslug/lib/${name}`)[name];
  sets[name] = new Set(codes);
  const r = ranges(codes);
  body += `\n/// uslug's \`lib/${name}.js\`: ${r.bmp} BMP code points of ${r.all}, in ${r.out.length} ranges.\n`;
  body += `pub(crate) const ${name}: &[(u16, u16)] = &[\n`;
  for (const [a, b] of r.out) body += `    (${hex(a)}, ${hex(b)}),\n`;
  body += '];\n';
}
// uslug tests L, N, Z, M in that order; the order only matters if Z overlaps.
for (const c of sets.Z) for (const o of ['L', 'N', 'M']) if (sets[o].has(c)) throw new Error(`Z overlaps ${o} at ${hex(c)}`);

const unormVersion = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'node_modules/unorm/package.json'), 'utf8')).version;
if (unormVersion !== '1.6.0') throw new Error(`unorm is ${unormVersion}, want 1.6.0`);
const unorm = from('unorm');
// UNORM_NO_COMPOSE is read from ICU: use the Unicode version Wiki.js's Node has.
if (process.versions.unicode !== '17.0') throw new Error(`Node has Unicode ${process.versions.unicode}, want 17.0 (ICU 78)`);
// The crate's own view (Unicode 18, a version past Node's ICU), read from its
// tables.rs the way gen-uslug-differential.js reads its classes, failing
// closed on anything unexpected: its canonical combining classes (each entry
// is code point << 8 | class) and the code points it decomposes canonically
// or by compatibility (each table is a perfect hash, as long as its salt).
const crate = (() => {
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
  const array = (name, type) => {
    const m = tables.match(new RegExp(`pub\\(crate\\) const ${name}: &\\[${type.replace(/[()[\]]/g, '\\$&')}\\] = &\\[([^\\]]*)\\];`));
    if (!m) throw new Error(`no ${name} in the crate`);
    return m[1];
  };
  const saltLen = (name) => array(name, 'u16').match(/0x[0-9A-F]+/g).length;
  const kv = array('CANONICAL_COMBINING_CLASS_KV', 'u32');
  const marks = new Map(kv.match(/0x[0-9A-Fa-f]+/g).map((h) => [parseInt(h, 16) >>> 8, parseInt(h, 16) & 0xff]));
  if (marks.size !== saltLen('CANONICAL_COMBINING_CLASS_SALT') || ![...marks.values()].every((k) => k > 0)) {
    throw new Error('misread the crate CCC table');
  }
  const keys = (name) => {
    const entries = array(`${name}_KV`, '(u32, (u16, u16))').match(/\(0x[0-9A-F]+, \(0x[0-9A-F]+, 0x[0-9A-F]+\)\)/g) || [];
    const set = new Set(entries.map((e) => parseInt(e.slice(1), 16)));
    if (set.size !== entries.length || set.size !== saltLen(`${name}_SALT`)) throw new Error(`misread the crate ${name} table`);
    return set;
  };
  // Its canonical compositions, composite -> [first, second]: the BMP pairs'
  // perfect hash (key first << 16 | second) and the match of the others.
  const composites = new Map();
  const pair = (u, a, b) => {
    if (composites.has(u)) throw new Error(`the crate composes ${hex(u)} twice`);
    composites.set(u, [a, b]);
  };
  const bmp = array('COMPOSITION_TABLE_KV', '(u32, char)').match(/\(0x[0-9A-F]{8}, '\\u\{[0-9A-F]+\}'\)/g) || [];
  if (bmp.length !== saltLen('COMPOSITION_TABLE_SALT')) throw new Error('misread the crate COMPOSITION_TABLE_KV');
  for (const e of bmp) {
    const [, k, u] = e.match(/^\(0x([0-9A-F]{8}), '\\u\{([0-9A-F]+)\}'\)$/);
    pair(parseInt(u, 16), parseInt(k, 16) >>> 16, parseInt(k, 16) & 0xffff);
  }
  const astral = tables.match(/pub\(crate\) fn composition_table_astral\(c1: char, c2: char\) -> Option<char> \{\n    match \(c1, c2\) \{\n([\s\S]*?)\n        _ => None,\n    \}\n\}/);
  if (!astral) throw new Error('no composition_table_astral in the crate');
  const arms = astral[1].split('\n');
  for (const arm of arms) {
    const m = arm.match(/^        \('\\u\{([0-9A-F]+)\}', '\\u\{([0-9A-F]+)\}'\) => Some\('\\u\{([0-9A-F]+)\}'\),$/);
    if (!m) throw new Error(`misread the crate composition_table_astral: ${arm}`);
    const [a, b, u] = m.slice(1).map((h) => parseInt(h, 16));
    if (a <= 0xffff && b <= 0xffff) throw new Error(`a BMP pair in composition_table_astral: ${arm}`);
    pair(u, a, b);
  }
  if (bmp.length < 900 || arms.length < 20) throw new Error('the crate composes suspiciously little');
  return { marks, canonical: keys('CANONICAL_DECOMPOSED'), compatibility: keys('COMPATIBILITY_DECOMPOSED'), composites };
})();
// The first character of the crate's full canonical decomposition of c: its
// compositions' first components, recursively, then ICU's (the crate
// decomposes nothing else canonically that ICU does not; checked below).
const crateFirst = (c) => crate.composites.has(c) ? crateFirst(crate.composites.get(c)[0]) : String.fromCodePoint(c).normalize('NFD').codePointAt(0);
const map = [];
for (let c = 0; c <= 0x10ffff; c++) {
  if (c >= 0xd800 && c <= 0xdfff) continue;
  const s = String.fromCodePoint(c);
  const theirs = unorm.nfkc(s);
  if (theirs !== s) map.push([c, theirs]);
}
// unorm's canonical combining classes, by probing: unorm has no API for them,
// but canonical reordering shows them. For a code point c that unorm does not
// decompose, with class k, NFD("a" + m + c) moves c before the mark m exactly
// when 0 < k < class(m). So k is nonzero iff c moves before U+0345 (240) or
// after U+0334 (1), and then k is the highest class among one representative
// mark per class that c does not move before. A code point unorm decomposes
// canonically and that has a nonzero class (U+0340, U+0341, U+0343, U+0344) is
// in UNORM_NFKC, which settles it on its own, so it is not probed.
const reps = [[1, 0x334], [7, 0x93c], [8, 0x3099], [9, 0x94d], [10, 0x5b0], [11, 0x5b1], [12, 0x5b2],
  [13, 0x5b3], [14, 0x5b4], [15, 0x5b5], [16, 0x5b6], [17, 0x5b7], [18, 0x5b8], [19, 0x5b9], [20, 0x5bb],
  [21, 0x5bc], [22, 0x5bd], [23, 0x5bf], [24, 0x5c1], [25, 0x5c2], [26, 0xfb1e], [27, 0x64b], [28, 0x64c],
  [29, 0x64d], [30, 0x64e], [31, 0x64f], [32, 0x650], [33, 0x651], [34, 0x652], [35, 0x670], [36, 0x711],
  [84, 0xc55], [91, 0xc56], [103, 0xe38], [107, 0xe48], [118, 0xeb8], [122, 0xec8], [129, 0xf71],
  [130, 0xf72], [132, 0xf74], [202, 0x321], [214, 0x1dce], [216, 0x31b], [218, 0x302a], [220, 0x316],
  [222, 0x59a], [224, 0x302e], [226, 0x1d16d], [228, 0x5ae], [230, 0x300], [232, 0x315], [233, 0x35c],
  [234, 0x35d], [240, 0x345]].map(([k, c]) => [k, String.fromCodePoint(c)]);
const movesBefore = (m, s) => unorm.nfd('a' + m + s) !== 'a' + m + s;
// The representatives themselves: each moves before exactly the higher ones.
for (const [k, m] of reps) for (const [j, n] of reps) {
  if (movesBefore(n, m) !== (k < j)) throw new Error(`unorm orders ${hex(m.codePointAt(0))} (${k}) and ${hex(n.codePointAt(0))} (${j}) unexpectedly`);
}
const ccc = [];
for (let c = 0; c <= 0x10ffff; c++) {
  if (c >= 0xd800 && c <= 0xdfff) continue;
  const s = String.fromCodePoint(c);
  if (unorm.nfd(s) !== s) continue;
  const above1 = unorm.nfd('a' + s + reps[0][1]) !== 'a' + s + reps[0][1];
  if (!above1 && !movesBefore(reps[reps.length - 1][1], s)) continue;
  let k = 0, moved = false;
  for (const [j, m] of reps) {
    if (!movesBefore(m, s)) {
      if (moved) throw new Error(`unorm orders ${hex(c)} non-monotonically`);
      k = j;
    } else moved = true;
  }
  ccc.push([c, k]);
}

// Canonical compositions unorm lacks. A primary composite (one that is its own
// NFC and decomposes canonically) that ICU forms and unorm does not is newer
// than unorm's data; NFC/NFKC in the crate would form it too. Its first
// component (a starter) is a code point unorm composes nothing with, so it
// goes into UNORM_NO_COMPOSE. Node's ICU can be a Unicode version behind the
// crate, so the crate's own compositions are read too and every one unorm does
// not form adds its first component as well (the union, from the crate's view);
// the unit test that walks every composite the crate knows (slug.rs) checks
// the result. UNORM_COMPOSITES, every non-Hangul primary composite unorm does
// form, is that test's reference (Hangul syllables are algorithmic in both,
// and checked separately).
const noCompose = new Set();
const unormComposites = [];
for (const [u, [a, b]] of crate.composites) {
  if (unorm.nfc(String.fromCodePoint(a, b)) !== String.fromCodePoint(u)) noCompose.add(crateFirst(a));
}
for (let c = 0; c <= 0x10ffff; c++) {
  if (c >= 0xd800 && c <= 0xdfff) continue;
  const s = String.fromCodePoint(c);
  const icuNfd = s.normalize('NFD');
  if ([...icuNfd].length >= 2 && icuNfd.normalize('NFC') === s && !(c >= 0xac00 && c <= 0xd7a3)) {
    const p = crate.composites.get(c);
    if (!p || String.fromCodePoint(...p).normalize('NFD') !== icuNfd) throw new Error(`ICU composes ${hex(c)} and the crate does not, or not alike`);
    if (unorm.nfc(icuNfd) !== s) noCompose.add(icuNfd.codePointAt(0));
  }
  const d = unorm.nfd(s);
  if ([...d].length >= 2 && unorm.nfc(d) === s && !(c >= 0xac00 && c <= 0xd7a3)) {
    if (d !== icuNfd) throw new Error(`unorm and ICU decompose ${hex(c)} differently`);
    unormComposites.push(c);
  }
}
for (const c of noCompose) {
  for (const u of unormComposites) {
    if ([...unorm.nfd(String.fromCodePoint(u))].some((x) => x.codePointAt(0) === c)) throw new Error(`${hex(c)} is in unorm's composite ${hex(u)}`);
  }
}
const noComposeSorted = [...noCompose].sort((a, b) => a - b);

// The code points the slugger splits at (slug.rs `splits_at`), predicted from
// the crate's view: those whose NFKC alone differs between the crate and unorm,
// whose class differs (unorm's class is only consulted for a code point unorm
// does not change on its own), or whose canonical decomposition starts with a
// UNORM_NO_COMPOSE character. The crate's NFKC alone is ICU's, except at a code
// point only the crate decomposes (Unicode 18): by compatibility, so its NFKC
// is not itself, where unorm leaves it alone. The unit test that checks S1
// requires the slugger to split at exactly these. Every other code point must
// have the same full NFKD in the crate as in unorm: UNORM_NFKD_SHA256 hashes
// unorm's, one line each, "<hex>\t<nfkd>\n", for the test to compare.
const isHangul = (c) => c >= 0xac00 && c <= 0xd7a3;
const unormCcc = new Map(ccc);
const split = [];
const nfkdHash = require('crypto').createHash('sha256');
for (let c = 0; c <= 0x10ffff; c++) {
  if (c >= 0xd800 && c <= 0xdfff) continue;
  const s = String.fromCodePoint(c);
  const theirs = unorm.nfkc(s);
  const icuDecomposes = s.normalize('NFKD') !== s && !isHangul(c);
  const crateDecomposes = crate.canonical.has(c) || crate.compatibility.has(c);
  if (icuDecomposes && !crateDecomposes) throw new Error(`ICU decomposes ${hex(c)} and the crate does not`);
  if (crate.canonical.has(c) && s.normalize('NFD') === s && !crate.composites.has(c)) {
    throw new Error(`the crate decomposes ${hex(c)} canonically and ICU does not, and it is no composite: extend the generator`);
  }
  let nfkcDiffers;
  if (crateDecomposes && !icuDecomposes) {
    if (theirs !== s) throw new Error(`unorm changes ${hex(c)}, which only the crate decomposes`);
    // A composite only the crate knows is its own NFKC, like any primary
    // composite; anything else only it decomposes is so by compatibility.
    nfkcDiffers = !crate.composites.has(c);
  } else nfkcDiffers = s.normalize('NFKC') !== theirs;
  const classDiffers = theirs === s && (crate.marks.get(c) || 0) !== (unormCcc.get(c) || 0);
  const startsNoCompose = noCompose.has(crateFirst(c));
  if (nfkcDiffers || classDiffers || startsNoCompose) split.push(c);
  else nfkdHash.update(`${c.toString(16)}\t${unorm.nfkd(s)}\n`);
}
const nfkdSha256 = nfkdHash.digest('hex');

const rustStr = (s) => '"' + [...s].map((ch) => `\\u{${ch.codePointAt(0).toString(16).toUpperCase()}}`).join('') + '"';
body += `\n/// Every code point unorm@1.6.0's NFKC changes, alone, with its result: ${map.length} entries. Sorted.\n`;
body += '/// Every other code point is its own NFKC under unorm.\n';
body += 'pub(crate) const UNORM_NFKC: &[(char, &str)] = &[\n';
for (const [c, t] of map) body += `    ('\\u{${c.toString(16).toUpperCase()}}', ${rustStr(t)}),\n`;
body += '];\n';
body += `\n/// Every code point unorm@1.6.0 does not decompose and gives a nonzero canonical\n`;
body += `/// combining class, with the class (probed, see tools/gen-uslug-tables.js): ${ccc.length} entries. Sorted.\n`;
body += '/// unorm treats every other code point as a starter (class 0).\n';
body += 'pub(crate) const UNORM_CCC: &[(char, u8)] = &[\n';
for (const [c, k] of ccc) body += `    ('\\u{${c.toString(16).toUpperCase()}}', ${k}),\n`;
body += '];\n';
const cp = (c) => `'\\u{${c.toString(16).toUpperCase()}}'`;
body += `\n/// The first components of the canonical composites ICU (Unicode ${process.versions.unicode}) forms and unorm@1.6.0\n`;
body += `/// does not: ${noComposeSorted.length} entries. Sorted. unorm composes nothing with these.\n`;
body += 'pub(crate) const UNORM_NO_COMPOSE: &[char] = &[\n';
for (const c of noComposeSorted) body += `    ${cp(c)},\n`;
body += '];\n';
body += `\n/// Every primary composite unorm@1.6.0 forms, Hangul syllables aside: ${unormComposites.length} entries.\n`;
body += '/// Sorted. Only the tests use it, as the reference the crate\'s composites are checked against.\n';
body += '#[cfg(test)]\n';
body += 'pub(crate) const UNORM_COMPOSITES: &[char] = &[\n';
for (const c of unormComposites) body += `    ${cp(c)},\n`;
body += '];\n';
body += `\n/// The code points the slugger splits at (slug.rs \`splits_at\`), predicted from unorm@1.6.0, ICU\n`;
body += `/// (Unicode ${process.versions.unicode}) and the unicode-normalization crate's tables (Unicode 18): ${split.length} entries. Sorted.\n`;
body += '/// Only the tests use it: the slugger must split at exactly these.\n';
body += '#[cfg(test)]\n';
body += 'pub(crate) const UNORM_SPLIT: &[char] = &[\n';
for (const c of split) body += `    ${cp(c)},\n`;
body += '];\n';
body += `\n/// SHA-256 of unorm@1.6.0's NFKD of every other code point, one line each, "<hex>\\t<nfkd>\\n"\n`;
body += `/// (${0x110000 - 0x800 - split.length} lines). Only the tests use it: the crate's NFKD must hash the same.\n`;
body += '#[cfg(test)]\n';
body += `pub(crate) const UNORM_NFKD_SHA256: &str =\n    "${nfkdSha256}";\n`;

const header = `// @generated by tools/gen-uslug-tables.js from uslug@1.0.4 and unorm@1.6.0 (npm). Do not edit.
//
// The category tables are derived from uslug's lib/{L,N,M,Z}.js, which carry this notice:
//
${license.split('\n').map((l) => ('// ' + l).trimEnd()).join('\n')}
//
// UNORM_NFKC, UNORM_CCC, UNORM_NO_COMPOSE, UNORM_COMPOSITES, UNORM_SPLIT and UNORM_NFKD_SHA256 are unorm@1.6.0's output, used under the same MIT terms:
// Copyright (c) 2008-2013 Matsuza <matsuza@gmail.com>, Bjarke Walling <bwp@bwp.dk>
// (unorm is dual-licensed MIT or GPL-2.0; see NOTICE).
`;
const out = path.join(repo, 'crates/rubrica-render/src/uslug_tables.rs');
fs.writeFileSync(out, header + body);
console.log(`wrote ${out}`);
