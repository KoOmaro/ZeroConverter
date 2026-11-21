// Lists every third-party package that ends up in the app or the command line,
// with its license and license text, and refuses licenses we cannot ship.
//
//   node scripts/licenses.mjs           writes licenses/THIRD_PARTY_LICENSES.txt
//   node scripts/licenses.mjs --check   only checks the licenses (CI)
//
// Rust: everything the app and the CLI link on Windows x64 (cargo metadata,
// normal and build dependencies). JavaScript: the runtime dependencies in
// package.json and everything they pull in. Identical license texts are
// printed once, with the packages that use them.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const checkOnly = process.argv.includes("--check");
const out = path.join(root, "licenses", "THIRD_PARTY_LICENSES.txt");

// GPL and AGPL (any version) cannot be linked into an MIT app we ship as a
// binary. LGPL is fine as used here (see THIRD_PARTY_NOTICES.md).
const forbidden = /(^|[^L])\b(A?GPL)-/i;
const licenseFile = /^(licen[cs]e|copying|notice|unlicense|copyright)([-._].*)?$/i;

function texts(dir, extra) {
  const files = new Set(extra ? [path.resolve(dir, extra)] : []);
  for (const name of fs.existsSync(dir) ? fs.readdirSync(dir) : []) {
    if (licenseFile.test(name) && fs.statSync(path.join(dir, name)).isFile()) files.add(path.join(dir, name));
  }
  return [...files].filter((f) => fs.existsSync(f)).sort().map((f) => fs.readFileSync(f, "utf8").replace(/\r\n/g, "\n").trim());
}

function rustPackages() {
  const meta = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--locked", "--filter-platform", "x86_64-pc-windows-msvc"],
      { cwd: path.join(root, "src-tauri"), maxBuffer: 256 << 20 },
    ).toString(),
  );
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const seen = new Set();
  const stack = meta.packages.filter((p) => ["zero-converter", "zeroconverter-cli"].includes(p.name)).map((p) => p.id);
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id).deps) {
      if (dep.dep_kinds.some((k) => k.kind === null || k.kind === "build")) stack.push(dep.pkg);
    }
  }
  return [...seen]
    .map((id) => byId.get(id))
    .filter((p) => p.source) // workspace members are ours
    .map((p) => ({
      name: p.name,
      version: p.version,
      license: p.license ?? "(see license text)",
      url: p.repository ?? "",
      texts: texts(path.dirname(p.manifest_path), p.license_file),
    }));
}

function npmPackages() {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
  const found = new Map();
  const visit = (name, from) => {
    // Node's lookup: the nearest node_modules up from the requiring package.
    let dir = from;
    let pdir;
    for (;;) {
      const candidate = path.join(dir, "node_modules", name);
      if (fs.existsSync(path.join(candidate, "package.json"))) {
        pdir = candidate;
        break;
      }
      const up = path.dirname(dir);
      if (up === dir || !up.startsWith(root)) return;
      dir = up;
    }
    if (found.has(pdir)) return;
    const p = JSON.parse(fs.readFileSync(path.join(pdir, "package.json"), "utf8"));
    const license = typeof p.license === "string" ? p.license : p.license?.type ?? "(see license text)";
    const url = typeof p.repository === "string" ? p.repository : p.repository?.url ?? "";
    found.set(pdir, { name: p.name, version: p.version, license, url, texts: texts(pdir) });
    for (const dep of Object.keys(p.dependencies ?? {})) visit(dep, pdir);
  };
  for (const dep of Object.keys(pkg.dependencies ?? {})) visit(dep, root);
  return [...found.values()];
}

const packages = [
  ...rustPackages().map((p) => ({ ...p, kind: "Rust crate" })),
  ...npmPackages().map((p) => ({ ...p, kind: "npm package" })),
].sort((a, b) => `${a.kind}${a.name}${a.version}`.localeCompare(`${b.kind}${b.name}${b.version}`));

// An expression is fine when at least one OR-alternative is shippable.
const refused = packages.filter((p) =>
  p.license
    .replace(/\//g, " OR ")
    .split(/\s+OR\s+/i)
    .every((alt) => forbidden.test(alt)),
);
if (refused.length) {
  console.error("Licenses that cannot ship with ZeroConverter:");
  for (const p of refused) console.error(`  ${p.kind} ${p.name} ${p.version}: ${p.license}`);
  process.exit(1);
}
const missing = packages.filter((p) => p.texts.length === 0);
console.log(`${packages.length} third-party packages, licenses OK (${missing.length} without a license file of their own).`);
if (checkOnly) process.exit(0);

// Group packages by identical license text so each text is printed once.
const groups = new Map();
for (const p of packages) {
  const body = p.texts.length
    ? p.texts.join("\n\n---\n\n")
    : `No license file is included in this package. Its license is ${p.license}; the standard text is in the licenses folder or at https://spdx.org/licenses/.`;
  const key = createHash("sha256").update(body).digest("hex");
  if (!groups.has(key)) groups.set(key, { body, users: [] });
  groups.get(key).users.push(p);
}

const lines = [
  "Third-party software in ZeroConverter",
  "=====================================",
  "",
  "ZeroConverter itself is MIT licensed (LICENSE.txt). It includes the packages",
  "below. THIRD_PARTY_NOTICES.md explains the ones with extra terms (LAME, FFmpeg).",
  "",
  "Packages",
  "--------",
  ...packages.map((p) => `${p.kind}: ${p.name} ${p.version} (${p.license})${p.url ? ` ${p.url}` : ""}`),
  "",
];
for (const { body, users } of groups.values()) {
  lines.push("=".repeat(78));
  lines.push(`Used by: ${users.map((u) => `${u.name} ${u.version}`).join(", ")}`);
  lines.push("=".repeat(78), "", body, "");
}
fs.writeFileSync(out, lines.join("\n"));
console.log(`Wrote ${path.relative(root, out)} (${groups.size} distinct license texts).`);
