#!/usr/bin/env node
"use strict";

// Launcher for the native `gwsr` binary shipped in this host's platform
// package (see ../lib/platform.js). Arguments, stdio and the exit status (or
// terminating signal) pass through unchanged. Nothing is downloaded: any
// resolution failure exits 1 with guidance on stderr and nothing on stdout.

const { spawnSync } = require("node:child_process");
const { ResolveError, resolveBinary } = require("../lib/platform.js");

function fail(message) {
  process.stderr.write(`gwsr: ${message}\n`);
  process.exit(1);
}

let binaryPath;
try {
  ({ binaryPath } = resolveBinary());
} catch (err) {
  if (err instanceof ResolveError) fail(err.message);
  throw err;
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  fail(`failed to run ${binaryPath}: ${result.error.message}`);
}
if (result.signal) {
  // Re-raise so callers observe the same signal the binary died from.
  process.kill(process.pid, result.signal);
  // Not reached for fatal signals; fall back to the shell convention.
  process.exit(1);
}
process.exit(result.status ?? 1);
