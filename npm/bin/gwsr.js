#!/usr/bin/env node
"use strict";

// Launcher for the native `gwsr` binary shipped in this host's platform
// package (see ../lib/platform.js). Arguments, stdio and the exit status (or
// terminating signal) pass through unchanged, and termination signals sent to
// this launcher are forwarded to the binary, so `kill`, `docker stop` or a
// supervisor reach gwsr instead of orphaning it. Nothing is downloaded: any
// resolution failure exits 1 with guidance on stderr and nothing on stdout.

const { spawn } = require("node:child_process");
const os = require("node:os");
const { ResolveError, resolveBinary } = require("../lib/platform.js");

// Signals a supervisor or user sends to stop a process. On Windows a console
// Ctrl+C/Ctrl+Break already reaches the child (same console); the launcher
// only has to survive it and wait for the child's own exit status.
const FORWARDED_SIGNALS =
  process.platform === "win32" ? ["SIGINT", "SIGBREAK"] : ["SIGINT", "SIGTERM", "SIGHUP", "SIGQUIT"];

const NOT_RERAISED = new Set(["SIGPIPE", "SIGUSR1"]);

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

// Install the forwarding handlers before spawning. Once the binary is running
// it can produce output, and whoever reads that output may signal the launcher
// straight away; if the default action still applied at that point, the
// launcher would die and orphan the binary. Node delivers signals to these
// handlers from the event loop, so none runs before `child` is assigned below.
let child;
const handlers = new Map();
for (const signal of FORWARDED_SIGNALS) {
  const handler = () => {
    // A child that already exited is ignored by kill(); its exit handler reports the status.
    if (process.platform !== "win32") child.kill(signal);
  };
  handlers.set(signal, handler);
  process.on(signal, handler);
}

child = spawn(binaryPath, process.argv.slice(2), { stdio: "inherit" });

function removeHandlers() {
  for (const [signal, handler] of handlers) process.removeListener(signal, handler);
}

child.on("error", (err) => {
  removeHandlers();
  fail(`failed to run ${binaryPath}: ${err.message}`);
});

child.on("exit", (code, signal) => {
  removeHandlers();
  if (signal) {
    // Re-raise so callers observe the same signal the binary died from. With
    // the forwarding handlers removed the default action applies. Node
    // ignores SIGPIPE and starts its inspector on SIGUSR1, so those (and any
    // signal that does not end this process) fall back to the shell
    // convention, 128 + signal number.
    if (!NOT_RERAISED.has(signal)) process.kill(process.pid, signal);
    process.exit(128 + (os.constants.signals[signal] ?? 0));
  }
  process.exit(code ?? 1);
});
