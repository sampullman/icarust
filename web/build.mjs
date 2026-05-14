#!/usr/bin/env node
// Build the Icarust wasm client and stage it under web/public for Vite.
//
//   node build.mjs                       # debug build
//   node build.mjs --release             # release build (smoother in browser)
//
// Adapted from ggez/examples/web/build.mjs — same shape (cargo build → wasm-bindgen
// → resources.zip) but for a single workspace crate instead of N examples.

import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync, cpSync, existsSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildStoredZip } from './src/zip.js';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..');
const PUBLIC = join(HERE, 'public');
const CLIENT_OUT = join(PUBLIC, 'client');
const RESOURCES_OUT = join(PUBLIC, 'resources');

const argv = process.argv.slice(2);
const flags = new Set(argv.filter(a => a.startsWith('--')));
const release = flags.has('--release');

// wasm-bindgen-cli version must match the one in Cargo.lock or the JS shim won't
// match what's compiled into the wasm.
const lock = readFileSync(join(REPO, 'Cargo.lock'), 'utf8');
const lockMatch = lock.match(/name = "wasm-bindgen"\nversion = "([^"]+)"/);
if (!lockMatch) {
  console.error('could not find wasm-bindgen version in Cargo.lock');
  process.exit(1);
}
const requiredBindgen = lockMatch[1];

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: REPO, ...opts });
  if (r.status !== 0) process.exit(r.status ?? 1);
  return r;
}

const bindgenProbe = spawnSync('wasm-bindgen', ['--version'], { stdio: 'pipe', cwd: REPO });
const installedBindgen = bindgenProbe.status === 0
  ? bindgenProbe.stdout.toString().trim().split(/\s+/).pop()
  : null;

if (installedBindgen !== requiredBindgen) {
  console.error(`wasm-bindgen ${requiredBindgen} required (have ${installedBindgen ?? 'none'}).`);
  console.error(`install with: cargo install --locked --version ${requiredBindgen} wasm-bindgen-cli`);
  process.exit(1);
}

const profileDir = release ? 'release' : 'debug';
const profileArgs = release ? ['--release'] : [];

mkdirSync(CLIENT_OUT, { recursive: true });

// `--lib` so we build the cdylib (the [[bin]] target is unused on web).
// ggez ripped out WebGL support; WebGPU is the only browser backend now.
const cargoArgs = [
  'build', '-p', 'client', '--lib', '--target', 'wasm32-unknown-unknown', ...profileArgs,
];
console.log(`\n→ cargo ${cargoArgs.join(' ')}`);
run('cargo', cargoArgs);

const wasmIn = join(REPO, 'target', 'wasm32-unknown-unknown', profileDir, 'client.wasm');
if (!existsSync(wasmIn)) {
  console.error(`expected ${wasmIn} but it is missing — did cargo build fail?`);
  process.exit(1);
}

rmSync(CLIENT_OUT, { recursive: true, force: true });
mkdirSync(CLIENT_OUT, { recursive: true });
console.log(`→ wasm-bindgen client`);
run('wasm-bindgen', [
  '--target', 'web',
  '--no-typescript',
  '--out-dir', CLIENT_OUT,
  '--out-name', 'client',
  wasmIn,
]);

// Stage /resources both as files and as a zip. The zip is the thing
// `Filesystem::new_web` mounts; the loose files would only matter if anything
// used `fetch('resources/foo')` directly, which Icarust doesn't.
rmSync(RESOURCES_OUT, { recursive: true, force: true });
cpSync(join(REPO, 'resources'), RESOURCES_OUT, { recursive: true });

function walkFiles(root, prefix = '') {
  const out = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const sub = prefix ? `${prefix}/${entry.name}` : entry.name;
    const abs = join(root, entry.name);
    if (entry.isDirectory()) {
      out.push(...walkFiles(abs, sub));
    } else if (entry.isFile()) {
      out.push({ path: sub, data: readFileSync(abs) });
    }
  }
  return out;
}
writeFileSync(join(PUBLIC, 'resources.zip'), buildStoredZip(walkFiles(join(REPO, 'resources'))));

console.log(`\n✓ built client into ${PUBLIC}`);
console.log(`  next: \`npm run serve\` (or \`npm run dev\` to rebuild + serve)`);
