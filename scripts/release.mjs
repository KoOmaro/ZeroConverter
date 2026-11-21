// Builds the signed installer and writes the files for a GitHub Release.
//
//   TAURI_SIGNING_PRIVATE_KEY=<path to key> node scripts/release.mjs
//
// Output in release/: the installer, its .sig, latest.json (what installed
// apps read to find the update), the command line as a zip with its license
// files, and SHA256SUMS.txt. Both carry licenses/THIRD_PARTY_LICENSES.txt,
// the full list of third-party packages, made fresh here. Upload all of them to a GitHub Release tagged
// v<version>, then publish it; apps pick it up within a day.
import { execFileSync, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const root = process.cwd();
const tauriDir = path.join(root, "src-tauri");
const conf = JSON.parse(fs.readFileSync(path.join(tauriDir, "tauri.conf.json"), "utf8"));
const version = conf.version;
const repo = "zerofoundry/ZeroConverter";

if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
  console.error("Set TAURI_SIGNING_PRIVATE_KEY to the updater key (path or contents).");
  process.exit(1);
}

// Where Cargo really puts things (CARGO_TARGET_DIR, config, or the default).
const target = JSON.parse(
  execSync("cargo metadata --format-version 1 --no-deps", { cwd: tauriDir, maxBuffer: 64 << 20 }).toString(),
).target_directory;

// Every third-party license, checked (no GPL or AGPL) and written to
// licenses/, which the installer and the CLI zip both carry.
const inventory = path.join(root, "licenses", "THIRD_PARTY_LICENSES.txt");
fs.rmSync(inventory, { force: true });
execFileSync(process.execPath, [path.join(root, "scripts", "licenses.mjs")], { stdio: "inherit" });
if (!fs.existsSync(inventory)) {
  console.error("licenses/THIRD_PARTY_LICENSES.txt was not written.");
  process.exit(1);
}

// The command line first: the installer carries it in its cli folder.
execSync("cargo build --release --locked -p zeroconverter-cli", { stdio: "inherit", cwd: tauriDir });
const cliExe = path.join(target, "release", "zeroconverter.exe");
const cliVersion = JSON.parse(execFileSync(cliExe, ["version", "--json"]).toString()).version;
if (cliVersion !== version) {
  console.error(`zeroconverter.exe reports ${cliVersion}, the app is ${version}. Align the versions first.`);
  process.exit(1);
}
fs.copyFileSync(cliExe, path.join(tauriDir, "cli", "zeroconverter.exe"));

// Release builds sign the update; contributor builds skip it (no key needed).
const releaseConf = path.join(os.tmpdir(), "zeroconverter.release.conf.json");
fs.writeFileSync(releaseConf, JSON.stringify({ bundle: { createUpdaterArtifacts: true } }));
execFileSync("npm", ["run", "tauri", "--", "build", "--config", releaseConf], {
  stdio: "inherit",
  shell: true,
});

const nsis = path.join(target, "release", "bundle", "nsis");
const installer = `${conf.productName}_${version}_x64-setup.exe`;
const exe = path.join(nsis, installer);
const sig = fs.readFileSync(`${exe}.sig`, "utf8").trim();

const out = path.join(root, "release");
fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
fs.copyFileSync(exe, path.join(out, installer));
fs.copyFileSync(`${exe}.sig`, path.join(out, `${installer}.sig`));

const notes = process.env.RELEASE_NOTES ?? `See https://github.com/${repo}/releases/tag/v${version}`;
const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: sig,
      url: `https://github.com/${repo}/releases/download/v${version}/${installer}`,
    },
  },
};
fs.writeFileSync(path.join(out, "latest.json"), JSON.stringify(latest, null, 2) + "\n");

// The command line on its own, with the same license files as the installer
// (LAME is linked in).
const cliName = `zeroconverter-cli_${version}_x64`;
const cliDir = path.join(out, cliName);
fs.mkdirSync(path.join(cliDir, "licenses"), { recursive: true });
fs.copyFileSync(cliExe, path.join(cliDir, "zeroconverter.exe"));
fs.copyFileSync(path.join(root, "LICENSE"), path.join(cliDir, "LICENSE.txt"));
fs.copyFileSync(path.join(root, "THIRD_PARTY_NOTICES.md"), path.join(cliDir, "THIRD_PARTY_NOTICES.md"));
for (const f of fs.readdirSync(path.join(root, "licenses"))) {
  fs.copyFileSync(path.join(root, "licenses", f), path.join(cliDir, "licenses", f));
}
const cliZip = path.join(out, `${cliName}.zip`);
execFileSync("powershell", [
  "-NoProfile",
  "-Command",
  `Compress-Archive -Path '${cliDir}\\*' -DestinationPath '${cliZip}' -Force`,
]);
fs.rmSync(cliDir, { recursive: true });

const sha256 = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const sha = sha256(exe);
fs.writeFileSync(
  path.join(out, "SHA256SUMS.txt"),
  `${sha}  ${installer}\n${sha256(cliZip)}  ${cliName}.zip\n`,
);

console.log(
  `\nv${version} ready in release/:\n  ${installer}\n  ${installer}.sig\n  latest.json\n  ${cliName}.zip\n  SHA256SUMS.txt\nSHA-256 ${sha}`,
);
