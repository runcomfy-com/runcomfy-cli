#!/usr/bin/env node
// Downloads the platform-native runcomfy binary from GitHub Releases and
// drops it into ./bin/. Verifies SHA-256 against the published .sha256.
//
// Skip with RUNCOMFY_SKIP_POSTINSTALL=1 (useful when this package is being
// vendored or the binary is supplied by another mechanism).

const fs = require('node:fs');
const path = require('node:path');
const https = require('node:https');
const crypto = require('node:crypto');
const os = require('node:os');
const { execSync } = require('node:child_process');

const REPO = 'runcomfy-com/runcomfy-cli';

const TARGETS = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
};

function detectTarget() {
  const key = `${process.platform}-${process.arch}`;
  const target = TARGETS[key];
  if (!target) {
    process.stderr.write(
      `runcomfy: unsupported platform/arch: ${key}\n` +
      `Supported: ${Object.keys(TARGETS).join(', ')}\n`
    );
    process.exit(1);
  }
  return target;
}

function getVersion() {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(__dirname, 'package.json'), 'utf8')
  );
  return pkg.version;
}

function followGet(url, redirectsLeft, onResponse, onError) {
  const req = https.get(
    url,
    { headers: { 'User-Agent': 'runcomfy-postinstall' } },
    (res) => {
      const code = res.statusCode || 0;
      if (code >= 300 && code < 400 && res.headers.location) {
        if (redirectsLeft <= 0) {
          res.resume();
          onError(new Error('too many redirects'));
          return;
        }
        res.resume();
        followGet(res.headers.location, redirectsLeft - 1, onResponse, onError);
        return;
      }
      if (code !== 200) {
        res.resume();
        onError(new Error(`HTTP ${code} for ${url}`));
        return;
      }
      onResponse(res);
    }
  );
  req.on('error', onError);
}

function downloadToFile(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    let settled = false;
    const fail = (err) => {
      if (settled) return;
      settled = true;
      file.destroy();
      try { fs.unlinkSync(dest); } catch (_) {}
      reject(err);
    };
    file.on('error', fail);
    followGet(
      url,
      5,
      (res) => {
        res.pipe(file);
        file.on('finish', () => {
          if (settled) return;
          settled = true;
          file.close(() => resolve());
        });
      },
      fail
    );
  });
}

function fetchText(url) {
  return new Promise((resolve, reject) => {
    followGet(
      url,
      5,
      (res) => {
        let body = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => { body += chunk; });
        res.on('end', () => resolve(body));
        res.on('error', reject);
      },
      reject
    );
  });
}

function sha256OfFile(file) {
  const hash = crypto.createHash('sha256');
  hash.update(fs.readFileSync(file));
  return hash.digest('hex');
}

async function main() {
  if (process.env.RUNCOMFY_SKIP_POSTINSTALL === '1') {
    process.stdout.write('runcomfy: postinstall skipped (RUNCOMFY_SKIP_POSTINSTALL=1)\n');
    return;
  }

  const target = detectTarget();
  const version = getVersion();
  const tag = `v${version}`;
  const tarballName = `runcomfy-${target}.tar.gz`;
  const releaseBase = `https://github.com/${REPO}/releases/download/${tag}`;
  const tarballUrl = `${releaseBase}/${tarballName}`;
  const sumUrl = `${tarballUrl}.sha256`;

  const binDir = path.join(__dirname, 'bin');
  fs.mkdirSync(binDir, { recursive: true });
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'runcomfy-'));
  const tarballPath = path.join(tmpDir, tarballName);

  try {
    process.stdout.write(`runcomfy: downloading ${tarballUrl}\n`);
    await downloadToFile(tarballUrl, tarballPath);

    process.stdout.write('runcomfy: verifying checksum\n');
    const sumBody = (await fetchText(sumUrl)).trim();
    const expected = sumBody.split(/\s+/)[0];
    const actual = sha256OfFile(tarballPath);
    if (!expected || expected !== actual) {
      process.stderr.write(
        `runcomfy: checksum mismatch\n` +
        `  expected: ${expected}\n` +
        `  actual:   ${actual}\n`
      );
      process.exit(1);
    }

    process.stdout.write('runcomfy: extracting\n');
    execSync(`tar -xzf "${tarballPath}" -C "${binDir}"`, { stdio: 'inherit' });
    const binPath = path.join(binDir, 'runcomfy');
    fs.chmodSync(binPath, 0o755);
    process.stdout.write(`runcomfy: installed ${binPath}\n`);
  } finally {
    try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch (_) {}
  }
}

main().catch((err) => {
  process.stderr.write(
    `runcomfy: postinstall failed: ${err.message}\n` +
    `You can install manually from https://github.com/runcomfy-com/runcomfy-cli/releases\n` +
    `Or skip this step with RUNCOMFY_SKIP_POSTINSTALL=1\n`
  );
  process.exit(1);
});
