import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const scriptsDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tmp = (prefix) => fs.mkdtempSync(path.join(os.tmpdir(), prefix));
const sha256 = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const run = (script, args, opts = {}) =>
  spawnSync("bash", [path.join(scriptsDir, script), ...args], { encoding: "utf8", ...opts });

function hasGnu(tool) {
  const res = spawnSync(tool, ["--version"], { encoding: "utf8" });
  return res.status === 0 && /GNU/.test(res.stdout);
}

describe("homebrew-formula.sh", () => {
  const version = "1.2.3";
  const targets = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
  ];
  const sums = (skip) => {
    const dir = tmp("gwsr-brew-");
    const file = path.join(dir, "SHA256SUMS");
    const lines = targets
      .filter((t) => t !== skip)
      .map((t, i) => `${String(i + 1).repeat(64)}  gws-rust-${version}-${t}.tar.gz`);
    fs.writeFileSync(file, `${lines.join("\n")}\n`);
    return file;
  };

  it("renders a formula with a URL and checksum per OS/CPU", () => {
    const res = run("homebrew-formula.sh", [version, sums()]);
    assert.equal(res.status, 0, res.stderr);
    assert.match(res.stdout, /class GwsRust < Formula/);
    assert.match(res.stdout, /bin\.install "gwsr"/);
    for (const [i, t] of targets.entries()) {
      assert.ok(
        res.stdout.includes(
          `url "https://github.com/astelmach20/gws-rust/releases/download/v${version}/gws-rust-${version}-${t}.tar.gz"`,
        ),
        t,
      );
      assert.ok(res.stdout.includes(`sha256 "${String(i + 1).repeat(64)}"`), t);
    }
  });

  it("fails when an archive is missing from SHA256SUMS", () => {
    const res = run("homebrew-formula.sh", [version, sums("x86_64-apple-darwin")]);
    assert.notEqual(res.status, 0);
    assert.match(res.stderr, /x86_64-apple-darwin/);
  });

  it("refuses prerelease versions", () => {
    const res = run("homebrew-formula.sh", ["1.2.3-rc.1", sums()]);
    assert.notEqual(res.status, 0);
    assert.match(res.stderr, /stable versions/);
  });
});

describe("package-release.sh", { skip: !(hasGnu("tar") && hasGnu("touch")) && "needs GNU tar and coreutils (CI runs on Linux)" }, () => {
  const bin = path.join(tmp("gwsr-bin-"), "gwsr");
  fs.writeFileSync(bin, "#!/bin/sh\necho gwsr\n", { mode: 0o755 });

  it("produces byte-identical archives across runs and records the fixed mtime", () => {
    const env = { ...process.env, SOURCE_DATE_EPOCH: "1700000000" };
    const outA = tmp("gwsr-pkg-a-");
    const outB = tmp("gwsr-pkg-b-");
    const a = run("package-release.sh", ["1.2.3", "x86_64-unknown-linux-musl", bin, outA], { env });
    assert.equal(a.status, 0, a.stderr);
    fs.utimesSync(bin, new Date(), new Date()); // a newer input mtime must not leak into the archive
    const b = run("package-release.sh", ["1.2.3", "x86_64-unknown-linux-musl", bin, outB], { env });
    assert.equal(b.status, 0, b.stderr);
    const name = "gws-rust-1.2.3-x86_64-unknown-linux-musl.tar.gz";
    assert.equal(sha256(path.join(outA, name)), sha256(path.join(outB, name)));

    const listing = execFileSync("tar", ["-tvzf", path.join(outA, name)], { encoding: "utf8" });
    assert.match(listing, /gws-rust-1\.2\.3-x86_64-unknown-linux-musl\/gwsr/);
    assert.match(listing, /gws-rust-1\.2\.3-x86_64-unknown-linux-musl\/LICENSE/);
    assert.match(listing, /2023-11-14/);
    assert.doesNotMatch(listing, /[a-z]+\/[a-z]+ .*runner/);
  });

  it("requires SOURCE_DATE_EPOCH", () => {
    const env = { ...process.env };
    delete env.SOURCE_DATE_EPOCH;
    const res = run("package-release.sh", ["1.2.3", "x86_64-unknown-linux-musl", bin, tmp("gwsr-pkg-")], { env });
    assert.notEqual(res.status, 0);
    assert.match(res.stderr, /SOURCE_DATE_EPOCH/);
  });
});

describe("check-changeset.sh", () => {
  function repo() {
    const dir = tmp("gwsr-policy-");
    const git = (...args) => execFileSync("git", args, { cwd: dir, encoding: "utf8" });
    git("init", "-q", "-b", "main");
    git("config", "user.email", "test@example.com");
    git("config", "user.name", "test");
    fs.mkdirSync(path.join(dir, "crates/gws-rust/src"), { recursive: true });
    fs.mkdirSync(path.join(dir, ".changeset"));
    fs.writeFileSync(path.join(dir, "crates/gws-rust/Cargo.toml"), '[dependencies]\ngws-rust-core = { version = "1", path = "../x" }\n');
    fs.writeFileSync(path.join(dir, "crates/gws-rust/src/main.rs"), "fn main() {}\n");
    fs.writeFileSync(path.join(dir, ".changeset/README.md"), "readme\n");
    git("add", ".");
    git("commit", "-q", "-m", "base");
    git("checkout", "-q", "-b", "feature");
    const write = (file, body) => {
      fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
      fs.writeFileSync(path.join(dir, file), body);
      git("add", file);
    };
    const commit = () => git("commit", "-q", "-m", "change");
    const check = () => spawnSync("bash", [path.join(scriptsDir, "check-changeset.sh"), "main"], { cwd: dir, encoding: "utf8" });
    return { write, commit, check };
  }

  it("requires a changeset when Rust code changes", () => {
    const r = repo();
    r.write("crates/gws-rust/src/main.rs", "fn main() { println!(); }\n");
    r.commit();
    const res = r.check();
    assert.equal(res.status, 1);
    assert.match(res.stdout, /adds no changeset/);
  });

  it("passes with a valid changeset", () => {
    const r = repo();
    r.write("crates/gws-rust/src/main.rs", "fn main() { println!(); }\n");
    r.write(".changeset/fix.md", '---\n"gws-rust": patch\n---\n\nFix a thing\n');
    r.commit();
    const res = r.check();
    assert.equal(res.status, 0, res.stdout + res.stderr);
  });

  it("does not require a changeset for non-Rust changes", () => {
    const r = repo();
    r.write("docs/guide.md", "hi\n");
    r.commit();
    assert.equal(r.check().status, 0);
  });

  it("rejects changesets that name another package", () => {
    const r = repo();
    r.write("crates/gws-rust/src/main.rs", "fn main() { println!(); }\n");
    r.write(".changeset/fix.md", '---\n"googleworkspace-cli": patch\n---\n\nFix\n');
    r.commit();
    const res = r.check();
    assert.equal(res.status, 1);
    assert.match(res.stdout, /Unknown package 'googleworkspace-cli'/);
  });

  it("rejects generated google-* API crates", () => {
    const r = repo();
    r.write("crates/gws-rust/Cargo.toml", '[dependencies]\ngoogle-drive3 = "5"\n');
    r.write(".changeset/fix.md", '---\n"gws-rust": patch\n---\n\nFix\n');
    r.commit();
    const res = r.check();
    assert.equal(res.status, 1);
    assert.match(res.stdout, /Generated google-\* crates are not allowed/);
  });
});
