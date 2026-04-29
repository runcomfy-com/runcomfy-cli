#!/usr/bin/env node
// Thin shim that delegates to the platform-native runcomfy binary.
// The binary is fetched into this directory by ../postinstall.js.

const { spawnSync } = require('node:child_process');
const path = require('node:path');
const fs = require('node:fs');

const ext = process.platform === 'win32' ? '.exe' : '';
const binPath = path.join(__dirname, `runcomfy${ext}`);

if (!fs.existsSync(binPath)) {
  process.stderr.write(
    `runcomfy: native binary not found at ${binPath}\n` +
    `The postinstall step may have failed. Try:\n` +
    `  npm rebuild @runcomfy/cli\n` +
    `Or download manually from https://github.com/runcomfy-com/runcomfy-cli/releases\n`
  );
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
  process.stderr.write(`runcomfy: failed to spawn binary: ${result.error.message}\n`);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
