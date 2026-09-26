import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const npmDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const { findPlatform, resolveBinary, ResolveError, platforms } = require("../lib/platform.js");
const mainManifest = require("../package.json");

function notFound() {
  const err = new Error("Cannot find module");
  err.code = "MODULE_NOT_FOUND";
  return err;
}

/** Lay out a fake platform package in a temp dir and return a resolver pointing at it. */
function fakePlatformPackage(entry, { version = mainManifest.version, withBinary = true } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "gwsr-platform-test-"));
  fs.mkdirSync(path.join(dir, "bin"));
  fs.writeFileSync(path.join(dir, "package.json"), JSON.stringify({ name: entry.package, version }));
  if (withBinary) fs.writeFileSync(path.join(dir, "bin", entry.binary), "");
  const resolve = (request) => {
    if (request === `${entry.package}/package.json`) return path.join(dir, "package.json");
    throw notFound();
  };
  return { dir, resolve };
}

describe("findPlatform", () => {
  it("maps every supported Node platform/arch pair to its package", () => {
    assert.equal(findPlatform("darwin", "arm64").package, "gws-rust-darwin-arm64");
    assert.equal(findPlatform("darwin", "x64").package, "gws-rust-darwin-x64");
    assert.equal(findPlatform("linux", "arm64").package, "gws-rust-linux-arm64");
    assert.equal(findPlatform("linux", "x64").package, "gws-rust-linux-x64");
  });

  it("has no npm package for Windows (use the release archive or cargo install)", () => {
    assert.equal(findPlatform("win32", "x64"), undefined);
    assert.equal(findPlatform("win32", "arm64"), undefined);
  });

  it("returns undefined for unsupported hosts", () => {
    assert.equal(findPlatform("freebsd", "x64"), undefined);
    assert.equal(findPlatform("linux", "ia32"), undefined);
    assert.equal(findPlatform("android", "arm64"), undefined);
  });
});

describe("package.json", () => {
  it("lists exactly the platform packages as optionalDependencies at its own version", () => {
    const expected = Object.fromEntries(platforms.map((p) => [p.package, mainManifest.version]));
    assert.deepEqual(mainManifest.optionalDependencies, expected);
  });

  it("has no install-time scripts and publishes to the public npm registry", () => {
    const scripts = mainManifest.scripts ?? {};
    for (const hook of ["preinstall", "install", "postinstall"]) {
      assert.equal(scripts[hook], undefined, `${hook} must not be defined`);
    }
    assert.equal(mainManifest.publishConfig.registry, "https://registry.npmjs.org/");
    assert.equal(mainManifest.publishConfig.provenance, true);
  });
});

describe("resolveBinary", () => {
  const entry = findPlatform("linux", "x64");

  it("returns the binary inside the installed platform package", () => {
    const { dir, resolve } = fakePlatformPackage(entry);
    const found = resolveBinary({ platform: "linux", arch: "x64", resolve });
    assert.equal(found.packageName, "gws-rust-linux-x64");
    assert.equal(found.binaryPath, path.join(dir, "bin", "gwsr"));
  });

  it("fails loudly on an unsupported platform", () => {
    assert.throws(
      () => resolveBinary({ platform: "sunos", arch: "x64", resolve: () => assert.fail("must not resolve") }),
      (err) => err instanceof ResolveError && /sunos-x64/.test(err.message) && /cargo install gws-rust/.test(err.message),
    );
  });

  it("explains how to reinstall when optional dependencies were skipped", () => {
    assert.throws(
      () => resolveBinary({ platform: "linux", arch: "x64", resolve: () => { throw notFound(); } }),
      (err) =>
        err instanceof ResolveError &&
        /gws-rust-linux-x64 is not installed/.test(err.message) &&
        /--omit=optional/.test(err.message),
    );
  });

  it("rejects a platform package whose version differs from the main package", () => {
    const { resolve } = fakePlatformPackage(entry, { version: "0.0.1-other" });
    assert.throws(
      () => resolveBinary({ platform: "linux", arch: "x64", resolve }),
      (err) => err instanceof ResolveError && /Version mismatch/.test(err.message),
    );
  });

  it("fails loudly when the package is present but the binary is missing", () => {
    const { resolve } = fakePlatformPackage(entry, { withBinary: false });
    assert.throws(
      () => resolveBinary({ platform: "linux", arch: "x64", resolve }),
      (err) => err instanceof ResolveError && /does not exist/.test(err.message),
    );
  });

  it("propagates unexpected resolver errors instead of masking them", () => {
    const boom = new Error("EACCES");
    assert.throws(() => resolveBinary({ platform: "linux", arch: "x64", resolve: () => { throw boom; } }), boom);
  });
});

describe("bin/gwsr.js", { skip: process.platform === "win32" && "uses a POSIX shell script as the fake binary" }, () => {
  const host = findPlatform(process.platform, process.arch);

  /** Install the main package and (optionally) the host platform package into a node_modules tree. */
  function install({ withPlatform, script }) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "gwsr-shim-test-"));
    const main = path.join(root, "node_modules", "gws-rust");
    fs.mkdirSync(main, { recursive: true });
    for (const item of ["bin", "lib", "platforms.json", "package.json"]) {
      fs.cpSync(path.join(npmDir, item), path.join(main, item), { recursive: true });
    }
    if (withPlatform) {
      const pkg = path.join(root, "node_modules", host.package);
      fs.mkdirSync(path.join(pkg, "bin"), { recursive: true });
      fs.writeFileSync(
        path.join(pkg, "package.json"),
        JSON.stringify({ name: host.package, version: mainManifest.version }),
      );
      fs.writeFileSync(path.join(pkg, "bin", host.binary), script, { mode: 0o755 });
    }
    return path.join(main, "bin", "gwsr.js");
  }

  it("passes arguments, stdout, stderr and the exit code through unchanged", { skip: !host && "unsupported host" }, () => {
    const script = '#!/bin/sh\nprintf \'{"args":"%s"}\\n\' "$*"\necho "diagnostic" >&2\nexit 7\n';
    const shim = install({ withPlatform: true, script });
    const res = spawnSync(process.execPath, [shim, "drive", "files", "list"], { encoding: "utf8" });
    assert.equal(res.stdout, '{"args":"drive files list"}\n');
    assert.equal(res.stderr, "diagnostic\n");
    assert.equal(res.status, 7);
  });

  it("passes stdin through to the binary", { skip: !host && "unsupported host" }, () => {
    const shim = install({ withPlatform: true, script: "#!/bin/sh\ncat\n" });
    const res = spawnSync(process.execPath, [shim], { input: '{"k":1}', encoding: "utf8" });
    assert.equal(res.stdout, '{"k":1}');
    assert.equal(res.status, 0);
  });

  it("exits non-zero with guidance and never downloads when the platform package is missing", { skip: !host && "unsupported host" }, () => {
    const shim = install({ withPlatform: false });
    const res = spawnSync(process.execPath, [shim, "--version"], { encoding: "utf8" });
    assert.equal(res.status, 1);
    assert.match(res.stderr, /is not installed/);
    assert.equal(res.stdout, "");
  });
});
