"use strict";

// Resolves the platform-specific package that carries the native `gwsr` binary.
//
// The main `gws-rust` package lists every platform package in `optionalDependencies`;
// npm/pnpm/yarn install only the one whose `os`/`cpu` match the host. Nothing is
// downloaded at install or run time: npm's own integrity checks cover the binary.

const fs = require("node:fs");
const path = require("node:path");

const { platforms } = require("../platforms.json");
const { version: mainVersion, name: mainName } = require("../package.json");

const RELEASES_URL = "https://github.com/astelmach20/gws-rust/releases";

class ResolveError extends Error {
  constructor(message) {
    super(message);
    this.name = "ResolveError";
  }
}

/**
 * Find the platform entry for a Node `process.platform` / `process.arch` pair.
 * Returns `undefined` when the host is not supported.
 */
function findPlatform(osName, cpu) {
  return platforms.find((p) => p.os === osName && p.cpu.includes(cpu));
}

function supportedList() {
  return platforms.map((p) => `${p.os}-${p.cpu.join("/")}`).join(", ");
}

function reinstallHint(pkg) {
  return [
    `Reinstall without skipping optional dependencies, e.g.:`,
    `  npm install -g ${mainName}@${mainVersion}`,
    `Common causes: installing with --omit=optional / --no-optional, or reusing a`,
    `node_modules directory or lockfile created on a different OS/CPU.`,
    `You can also install ${pkg} directly, run \`cargo install ${mainName}\`,`,
    `or download a release archive from ${RELEASES_URL}.`,
  ].join("\n");
}

/**
 * Locate the native binary for this host.
 *
 * @param {object} [opts]
 * @param {string} [opts.platform] defaults to process.platform
 * @param {string} [opts.arch] defaults to process.arch
 * @param {(request: string) => string} [opts.resolve] module resolver (defaults to require.resolve)
 * @returns {{ binaryPath: string, packageName: string }}
 * @throws {ResolveError} with an actionable message when anything is missing.
 */
function resolveBinary(opts = {}) {
  const osName = opts.platform ?? process.platform;
  const cpu = opts.arch ?? process.arch;
  const resolve = opts.resolve ?? require.resolve;

  const entry = findPlatform(osName, cpu);
  if (!entry) {
    throw new ResolveError(
      `gwsr does not ship a prebuilt binary for ${osName}-${cpu}.\n` +
        `Supported platforms: ${supportedList()}.\n` +
        `Download a release archive from ${RELEASES_URL} or build from source with \`cargo install ${mainName}\`.`,
    );
  }

  let manifestPath;
  try {
    manifestPath = resolve(`${entry.package}/package.json`);
  } catch (err) {
    if (err && err.code === "MODULE_NOT_FOUND") {
      throw new ResolveError(
        `The platform package ${entry.package} is not installed, so the gwsr binary is missing.\n` +
          reinstallHint(entry.package),
      );
    }
    throw err;
  }

  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  if (manifest.version !== mainVersion) {
    throw new ResolveError(
      `Version mismatch: ${mainName}@${mainVersion} found ${entry.package}@${manifest.version}.\n` +
        reinstallHint(entry.package),
    );
  }

  const binaryPath = path.join(path.dirname(manifestPath), "bin", entry.binary);
  if (!fs.existsSync(binaryPath)) {
    throw new ResolveError(
      `${entry.package}@${manifest.version} is installed but ${binaryPath} does not exist.\n` +
        reinstallHint(entry.package),
    );
  }

  return { binaryPath, packageName: entry.package };
}

module.exports = { ResolveError, findPlatform, resolveBinary, platforms };
