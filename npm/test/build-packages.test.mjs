import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";

import { archiveName, buildPackages, BuildError, parseChecksums } from "../scripts/build-packages.mjs";

const require = createRequire(import.meta.url);
const { platforms } = require("../platforms.json");
const { version } = require("../package.json");

const tmp = (prefix) => fs.mkdtempSync(path.join(os.tmpdir(), prefix));
const sha256 = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");

/** Create fake release archives (one per platform) and a matching SHA256SUMS. */
function fakeRelease({ corrupt } = {}) {
  const dir = tmp("gwsr-artifacts-");
  const lines = [];
  for (const p of platforms) {
    const name = archiveName(version, p.target);
    const stem = name.replace(/\.(tar\.gz|zip)$/, "");
    const src = tmp("gwsr-src-");
    fs.mkdirSync(path.join(src, stem));
    fs.writeFileSync(path.join(src, stem, p.binary), `binary for ${p.target}\n`);
    fs.writeFileSync(path.join(src, stem, "LICENSE"), "license\n");
    const archive = path.join(dir, name);
    if (name.endsWith(".zip")) {
      execFileSync("zip", ["-q", "-r", archive, stem], { cwd: src });
    } else {
      execFileSync("tar", ["-czf", archive, stem], { cwd: src });
    }
    lines.push(`${sha256(archive)}  ${name}`);
  }
  if (corrupt) fs.appendFileSync(path.join(dir, archiveName(version, corrupt)), "tampered");
  fs.writeFileSync(path.join(dir, "SHA256SUMS"), `${lines.join("\n")}\n`);
  return dir;
}

describe("parseChecksums", () => {
  it("parses text and binary mode lines", () => {
    const sums = parseChecksums(`${"a".repeat(64)}  one.tar.gz\n${"B".repeat(64)} *two.zip\n\n`);
    assert.equal(sums.get("one.tar.gz"), "a".repeat(64));
    assert.equal(sums.get("two.zip"), "b".repeat(64));
  });

  it("rejects malformed lines", () => {
    assert.throws(() => parseChecksums("not-a-hash  file\n"), BuildError);
  });
});

describe("buildPackages", () => {
  it("builds one package per platform plus the main package, in publish order", () => {
    const out = path.join(tmp("gwsr-out-"), "packages");
    const order = buildPackages({ version, artifacts: fakeRelease(), out });

    assert.deepEqual(order, [...platforms.map((p) => p.package), "gws-rust"]);
    assert.equal(fs.readFileSync(path.join(out, "publish-order.txt"), "utf8"), `${order.join("\n")}\n`);

    for (const p of platforms) {
      const dir = path.join(out, p.package);
      const manifest = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
      assert.equal(manifest.name, p.package);
      assert.equal(manifest.version, version);
      assert.deepEqual(manifest.os, [p.os]);
      assert.deepEqual(manifest.cpu, p.cpu);
      assert.equal(manifest.scripts, undefined);
      const bin = path.join(dir, "bin", p.binary);
      assert.equal(fs.readFileSync(bin, "utf8"), `binary for ${p.target}\n`);
      if (process.platform !== "win32") assert.equal(fs.statSync(bin).mode & 0o777, 0o755);
    }

    const main = path.join(out, "gws-rust");
    for (const f of ["bin/gwsr.js", "lib/platform.js", "platforms.json", "package.json", "README.md", "LICENSE", "NOTICE"]) {
      assert.ok(fs.existsSync(path.join(main, f)), f);
    }
  });

  it("refuses an archive whose checksum does not match SHA256SUMS", () => {
    const artifacts = fakeRelease({ corrupt: "x86_64-unknown-linux-musl" });
    assert.throws(
      () => buildPackages({ version, artifacts, out: path.join(tmp("gwsr-out-"), "p") }),
      (err) => err instanceof BuildError && /checksum mismatch/.test(err.message),
    );
  });

  it("refuses a version that does not match npm/package.json", () => {
    assert.throws(
      () => buildPackages({ version: "99.0.0", artifacts: fakeRelease(), out: path.join(tmp("gwsr-out-"), "p") }),
      (err) => err instanceof BuildError && /version-sync/.test(err.message),
    );
  });

  it("refuses when an archive is missing", () => {
    const artifacts = fakeRelease();
    fs.rmSync(path.join(artifacts, archiveName(version, "aarch64-apple-darwin")));
    assert.throws(
      () => buildPackages({ version, artifacts, out: path.join(tmp("gwsr-out-"), "p") }),
      (err) => err instanceof BuildError && /release archive not found/.test(err.message),
    );
  });
});
