#!/usr/bin/env node
// Builds the publishable npm packages from verified release archives.
//
//   node npm/scripts/build-packages.mjs --version 1.2.3 --artifacts dist --out npm/dist
//
// Produces one directory per platform package plus the main `gws-rust` package, and
// writes `publish-order.txt` (platform packages first, main package last). Every archive
// is checked against SHA256SUMS before its binary is used. Any inconsistency is fatal.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const npmDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(npmDir, "..");

export class BuildError extends Error {}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, value) {
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

export function archiveName(version, target) {
  const ext = target.includes("windows") ? "zip" : "tar.gz";
  return `gws-rust-${version}-${target}.${ext}`;
}

/** Parse a `sha256sum`-style file into a Map of file name -> lowercase hex digest. */
export function parseChecksums(text) {
  const sums = new Map();
  for (const [i, raw] of text.split("\n").entries()) {
    const line = raw.trim();
    if (line === "") continue;
    const match = /^([0-9a-fA-F]{64}) [ *](.+)$/.exec(line);
    if (!match) throw new BuildError(`SHA256SUMS line ${i + 1} is malformed: ${line}`);
    sums.set(match[2], match[1].toLowerCase());
  }
  return sums;
}

function sha256File(file) {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function extract(archive, dest) {
  if (archive.endsWith(".zip")) {
    execFileSync("unzip", ["-q", archive, "-d", dest], { stdio: "inherit" });
  } else {
    execFileSync("tar", ["-xzf", archive, "-C", dest], { stdio: "inherit" });
  }
}

function copyFile(from, toDir, name = path.basename(from)) {
  if (!fs.existsSync(from)) throw new BuildError(`required file is missing: ${from}`);
  fs.copyFileSync(from, path.join(toDir, name));
}

function platformReadme(entry, version) {
  return [
    `# ${entry.package}`,
    "",
    `The \`gwsr\` ${version} binary for ${entry.os} (${entry.cpu.join(", ")}), built for \`${entry.target}\`.`,
    "",
    "Do not install this package directly: install [`gws-rust`](https://www.npmjs.com/package/gws-rust),",
    "which selects the right platform package automatically.",
    "",
  ].join("\n");
}

export function buildPackages({ version, artifacts, out }) {
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new BuildError(`--version must be a semver version without a leading "v": ${version}`);
  }
  const mainManifest = readJson(path.join(npmDir, "package.json"));
  const { platforms } = readJson(path.join(npmDir, "platforms.json"));

  if (mainManifest.version !== version) {
    throw new BuildError(
      `npm/package.json is at ${mainManifest.version} but --version is ${version}; run scripts/version-sync.sh`,
    );
  }
  const expectedOptional = Object.fromEntries(platforms.map((p) => [p.package, version]));
  const actualOptional = mainManifest.optionalDependencies ?? {};
  const sortKeys = (o) => JSON.stringify(Object.fromEntries(Object.entries(o).sort()));
  if (sortKeys(actualOptional) !== sortKeys(expectedOptional)) {
    throw new BuildError(
      `npm/package.json optionalDependencies must be exactly ${sortKeys(expectedOptional)}, got ${sortKeys(actualOptional)}`,
    );
  }

  const sumsFile = path.join(artifacts, "SHA256SUMS");
  if (!fs.existsSync(sumsFile)) throw new BuildError(`${sumsFile} not found`);
  const sums = parseChecksums(fs.readFileSync(sumsFile, "utf8"));

  if (fs.existsSync(out)) fs.rmSync(out, { recursive: true });
  fs.mkdirSync(out, { recursive: true });
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "gwsr-npm-"));
  const order = [];

  try {
    for (const entry of platforms) {
      const name = archiveName(version, entry.target);
      const archive = path.join(artifacts, name);
      if (!fs.existsSync(archive)) throw new BuildError(`release archive not found: ${archive}`);
      const expected = sums.get(name);
      if (!expected) throw new BuildError(`SHA256SUMS has no entry for ${name}`);
      const actual = sha256File(archive);
      if (actual !== expected) {
        throw new BuildError(`checksum mismatch for ${name}: expected ${expected}, got ${actual}`);
      }

      const extractDir = path.join(work, entry.target);
      fs.mkdirSync(extractDir);
      extract(archive, extractDir);
      const stem = name.replace(/\.(tar\.gz|zip)$/, "");
      const binary = path.join(extractDir, stem, entry.binary);
      if (!fs.existsSync(binary)) throw new BuildError(`${name} does not contain ${stem}/${entry.binary}`);

      const pkgDir = path.join(out, entry.package);
      fs.mkdirSync(path.join(pkgDir, "bin"), { recursive: true });
      const dest = path.join(pkgDir, "bin", entry.binary);
      fs.copyFileSync(binary, dest);
      fs.chmodSync(dest, 0o755);
      copyFile(path.join(repoRoot, "LICENSE"), pkgDir);
      fs.writeFileSync(path.join(pkgDir, "README.md"), platformReadme(entry, version));
      writeJson(path.join(pkgDir, "package.json"), {
        name: entry.package,
        version,
        description: `The gwsr binary for ${entry.os}-${entry.cpu.join("/")}. Install gws-rust instead.`,
        license: mainManifest.license,
        repository: mainManifest.repository,
        homepage: mainManifest.homepage,
        os: [entry.os],
        cpu: entry.cpu,
        files: ["bin/"],
        preferUnplugged: true,
        publishConfig: mainManifest.publishConfig,
      });
      order.push(entry.package);
    }

    const mainDir = path.join(out, mainManifest.name);
    fs.mkdirSync(mainDir);
    fs.cpSync(path.join(npmDir, "bin"), path.join(mainDir, "bin"), { recursive: true });
    fs.cpSync(path.join(npmDir, "lib"), path.join(mainDir, "lib"), { recursive: true });
    copyFile(path.join(npmDir, "platforms.json"), mainDir);
    copyFile(path.join(npmDir, "package.json"), mainDir);
    copyFile(path.join(repoRoot, "README.md"), mainDir);
    copyFile(path.join(repoRoot, "LICENSE"), mainDir);
    copyFile(path.join(repoRoot, "NOTICE"), mainDir);
    fs.chmodSync(path.join(mainDir, "bin", "gwsr.js"), 0o755);
    order.push(mainManifest.name);
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }

  fs.writeFileSync(path.join(out, "publish-order.txt"), `${order.join("\n")}\n`);
  return order;
}

function main() {
  const { values } = parseArgs({
    options: {
      version: { type: "string" },
      artifacts: { type: "string" },
      out: { type: "string" },
    },
    strict: true,
  });
  for (const key of ["version", "artifacts", "out"]) {
    if (!values[key]) throw new BuildError(`missing required --${key}`);
  }
  const order = buildPackages({
    version: values.version,
    artifacts: path.resolve(values.artifacts),
    out: path.resolve(values.out),
  });
  for (const name of order) process.stdout.write(`built ${name}\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (err) {
    process.stderr.write(`error: ${err instanceof BuildError ? err.message : err.stack}\n`);
    process.exit(1);
  }
}
