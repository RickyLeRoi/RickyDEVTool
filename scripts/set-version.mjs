#!/usr/bin/env node

//   - package.json                 (npm)
//   - src-tauri/Cargo.toml         (crate + env!("CARGO_PKG_VERSION") in /api/health)
//   - src-tauri/tauri.conf.json    (bundle + updater/latest.json)
//   - src-tauri/Cargo.lock         (voce del crate, per non lasciare il lock sporco)

//   node scripts/set-version.mjs           # legge VERSION e sincronizza i file
//   node scripts/set-version.mjs 1.2.3     # scrive 1.2.3 in VERSION, poi sincronizza
//   node scripts/set-version.mjs --check   # non scrive: esce !=0 se qualcosa è disallineato

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const VERSION_FILE = join(ROOT, "VERSION");
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

const args = process.argv.slice(2);
const check = args.includes("--check");
const explicit = args.find((a) => !a.startsWith("--"));

const version = (explicit ?? readMaybe(VERSION_FILE)?.trim() ?? "").trim();
if (!SEMVER.test(version)) {
  fail(`Versione non valida: "${version}". Attesa x.y.z (semver, con eventuale -suffix).`);
}

// Ogni target sa leggere la propria versione attuale e riscriversi col nuovo valore.
const targets = [
  {
    file: "VERSION",
    read: (s) => s.trim(),
    write: () => `${version}\n`,
  },
  {
    file: "package.json",
    read: (s) => JSON.parse(s).version,
    write: (s) => s.replace(/("version"\s*:\s*")[^"]*(")/, `$1${version}$2`),
  },
  {
    file: "src-tauri/tauri.conf.json",
    read: (s) => JSON.parse(s).version,
    write: (s) => s.replace(/("version"\s*:\s*")[^"]*(")/, `$1${version}$2`),
  },
  {
    file: "src-tauri/Cargo.toml",
    read: (s) => (s.match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]*)"/m) ?? [])[1],
    write: (s) => s.replace(/(^\[package\][\s\S]*?^version\s*=\s*")[^"]*(")/m, `$1${version}$2`),
  },
  {
    file: "src-tauri/Cargo.lock",
    optional: true, // rigenerato comunque al prossimo build, ma lo teniamo pulito
    read: (s) => (s.match(/name = "rickydevtool"\nversion = "([^"]*)"/) ?? [])[1],
    write: (s) => s.replace(/(name = "rickydevtool"\nversion = ")[^"]*(")/, `$1${version}$2`),
  },
];

const changed = [];
const mismatched = [];

for (const t of targets) {
  const path = join(ROOT, t.file);
  const content = readMaybe(path);
  if (content == null) {
    if (t.optional) continue;
    fail(`Impossibile leggere ${t.file}`);
  }
  const current = t.read(content);
  if (current === version) continue;

  if (check) {
    mismatched.push(`${t.file}: ${current ?? "?"} ≠ ${version}`);
    continue;
  }
  const next = t.write(content);
  if (t.read(next) !== version) {
    fail(`Non sono riuscito ad aggiornare ${t.file} (campo versione non trovato).`);
  }
  writeFileSync(path, next);
  changed.push(t.file);
}

if (check) {
  if (mismatched.length) {
    fail(`Versioni disallineate rispetto a VERSION (${version}):\n  ${mismatched.join("\n  ")}`);
  }
  console.log(`✓ Tutti i file sono allineati a ${version}.`);
} else {
  console.log(
    changed.length
      ? `✓ Versione ${version} propagata a:\n  ${changed.join("\n  ")}`
      : `✓ Tutto già a ${version}: niente da aggiornare.`,
  );
}

function readMaybe(path) {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return null;
  }
}

function fail(message) {
  console.error(`✗ ${message}`);
  process.exit(1);
}
