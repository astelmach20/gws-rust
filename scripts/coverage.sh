#!/usr/bin/env bash
# Runs the workspace tests under cargo-llvm-cov and enforces the line-coverage floor.
#
#   scripts/coverage.sh            # text summary + HTML report in target/llvm-cov/html
#   scripts/coverage.sh --open     # ...and open the HTML report
#   scripts/coverage.sh --lcov     # write lcov.info instead of HTML (used by CI)
#
# The floor is COVERAGE_MIN_LINES (default below). Raise it as coverage improves; never lower it
# to make a PR pass.
set -euo pipefail

readonly DEFAULT_MIN_LINES=65
min_lines="${COVERAGE_MIN_LINES:-$DEFAULT_MIN_LINES}"
mode=html
open_report=false

for arg in "$@"; do
  case "$arg" in
    --open) open_report=true ;;
    --lcov) mode=lcov ;;
    -h | --help)
      sed -n '2,9p' "$0"
      exit 0
      ;;
    *)
      echo "error: unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "error: cargo-llvm-cov is not installed. Install it with 'cargo install cargo-llvm-cov --locked'" >&2
  echo "       (or enter 'nix develop', which provides it)." >&2
  exit 1
fi

cargo llvm-cov clean --workspace
cargo llvm-cov --workspace --all-features --locked --no-report

if [[ "$mode" == lcov ]]; then
  cargo llvm-cov report --lcov --output-path lcov.info
  echo "Wrote lcov.info"
else
  cargo llvm-cov report --html
  echo "HTML report: target/llvm-cov/html/index.html"
fi

cargo llvm-cov report --summary-only --fail-under-lines "$min_lines"
echo "Line coverage is at or above the ${min_lines}% floor."

if [[ "$open_report" == true && "$mode" == html ]]; then
  case "$(uname -s)" in
    Darwin) open target/llvm-cov/html/index.html ;;
    Linux) xdg-open target/llvm-cov/html/index.html ;;
    *) echo "Open target/llvm-cov/html/index.html in a browser." ;;
  esac
fi
