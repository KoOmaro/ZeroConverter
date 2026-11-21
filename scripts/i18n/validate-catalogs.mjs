// Checks that every interface language has exactly the keys English has, with the
// same {{placeholders}}, the right plural forms, and the names that must not be
// translated. Also checks that every literal t("key") in src/ exists.
// Run: node scripts/i18n/validate-catalogs.mjs
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const dir = path.join(root, "src", "i18n", "locales");
export const CODES = ["en", "tr", "de", "es", "fr", "pt-BR", "ja", "ko", "zh-Hans", "ru", "hi", "ar"];
const PLURAL = /_(zero|one|two|few|many|other)$/;
// Only Latin letters, digits and "_" glue to a name; Korean particles ("ICO는") and
// the Arabic "و" ("وHDR") sit right next to it and are fine.
const WORD = /[A-Za-z0-9_]/;
/** Product and technical names that stay as written in every language. */
const PROTECTED = [
  "ICO", "HDR", "FFmpeg", "ZeroConverter", "ZeroFoundry", "MP3", "LAME", "LGPL", "MIT", "PATH", "GitHub",
  "Windows", "Inter", "JetBrains Mono", "Space Grotesk", "node_modules", "Intermediate", "Library", "px", "MB",
];

const errors = [];
const fail = (code, msg) => errors.push(`${code}: ${msg}`);

function flatten(obj, prefix = "", out = {}) {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object") flatten(v, key, out);
    else out[key] = v;
  }
  return out;
}
const vars = (s) => new Set([...String(s).matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map((m) => m[1]));
const base = (k) => k.replace(PLURAL, "");

/** True when `name` appears as a whole word (not inside another word). */
function hasName(text, name) {
  let from = 0;
  for (;;) {
    const i = text.indexOf(name, from);
    if (i < 0) return false;
    const before = i === 0 ? "" : text[i - 1];
    const after = text[i + name.length] ?? "";
    if (!WORD.test(before) && !WORD.test(after)) return true;
    from = i + 1;
  }
}

const catalogs = {};
for (const code of CODES) {
  const file = path.join(dir, `${code}.json`);
  try {
    catalogs[code] = flatten(JSON.parse(fs.readFileSync(file, "utf8")));
  } catch (e) {
    fail(code, `cannot read ${file}: ${e.message}`);
  }
}
const en = catalogs.en;
if (!en) {
  console.error(errors.join("\n"));
  process.exit(1);
}

// Group English keys by base key.
const enGroups = new Map();
for (const k of Object.keys(en)) {
  const b = base(k);
  if (!enGroups.has(b)) enGroups.set(b, []);
  enGroups.get(b).push(k);
}
const unionVars = (cat, keys) => {
  const s = new Set();
  for (const k of keys) for (const v of vars(cat[k])) s.add(v);
  s.delete("count");
  return s;
};

for (const code of CODES) {
  const cat = catalogs[code];
  if (!cat) continue;
  const categories = new Set(new Intl.PluralRules(code).resolvedOptions().pluralCategories);
  const groups = new Map();
  for (const k of Object.keys(cat)) {
    const b = base(k);
    if (!groups.has(b)) groups.set(b, []);
    groups.get(b).push(k);
    if (typeof cat[k] !== "string" || (cat[k].trim() === "" && k !== "native.sentenceJoin")) fail(code, `empty value: ${k}`);
  }
  for (const [b, enKeys] of enGroups) {
    const keys = groups.get(b);
    if (!keys) {
      fail(code, `missing key: ${b}`);
      continue;
    }
    const enPlural = enKeys.some((k) => PLURAL.test(k));
    if (enPlural) {
      const have = new Set(keys.map((k) => k.match(PLURAL)?.[1]).filter(Boolean));
      for (const c of categories) if (!have.has(c)) fail(code, `missing plural form ${b}_${c}`);
      for (const c of have) if (!categories.has(c)) fail(code, `unused plural form ${b}_${c}`);
    } else if (keys.length !== 1 || keys[0] !== b) {
      fail(code, `${b} must be a single string`);
    }
    const want = [...unionVars(en, enKeys)].sort();
    for (const k of keys) {
      const got = [...vars(cat[k])].filter((v) => v !== "count").sort();
      // A plural form may leave the number out ("one file"); the others need every placeholder.
      const same = got.join(",") === want.join(",");
      const subset = PLURAL.test(k) && got.every((v) => want.includes(v));
      if (!same && !subset) fail(code, `placeholders differ in ${k}: {{${got.join("}}, {{")}}} vs English {{${want.join("}}, {{")}}}`);
    }
    // Names that stay as written.
    const enText = enKeys.map((k) => en[k]).join(" ");
    const text = keys.map((k) => cat[k]).join(" ");
    for (const name of PROTECTED) {
      if (hasName(enText, name) && !hasName(text, name)) fail(code, `"${name}" must stay in ${b}`);
    }
  }
  for (const b of groups.keys()) if (!enGroups.has(b)) fail(code, `unknown key: ${b}`);
}

// Every literal t("key") in the source exists in English.
function walk(d, out = []) {
  for (const f of fs.readdirSync(d)) {
    const p = path.join(d, f);
    if (fs.statSync(p).isDirectory()) walk(p, out);
    else if (/\.(ts|tsx)$/.test(f)) out.push(p);
  }
  return out;
}
for (const file of walk(path.join(root, "src"))) {
  const src = fs.readFileSync(file, "utf8");
  for (const m of src.matchAll(/\b(?:i18n\.)?t\(\s*"([\w.-]+)"/g)) {
    if (!enGroups.has(m[1])) fail("src", `${path.relative(root, file)} uses unknown key ${m[1]}`);
  }
}

if (errors.length) {
  console.error(`i18n: ${errors.length} problem(s)\n` + errors.join("\n"));
  process.exit(1);
}
console.log(`i18n: ${CODES.length} languages, ${enGroups.size} strings, all in step.`);
