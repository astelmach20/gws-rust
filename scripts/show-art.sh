#!/usr/bin/env bash
# Clears the terminal and prints an ASCII-art title card (used by docs/demo.tape).
#
#   scripts/show-art.sh art/intro.txt
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <art-file>" >&2
  exit 2
fi
if [[ ! -r "$1" ]]; then
  echo "error: cannot read art file: $1" >&2
  exit 1
fi

clear
cat -- "$1"
