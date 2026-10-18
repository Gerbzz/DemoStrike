#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
commit=7567c24ec2501a7b07cda6213ece4540e57b5039
mkdir -p reference/pinned
curl -fsSL "https://raw.githubusercontent.com/advancedfx/advancedfx/$commit/shared/CommandSystem.cpp" -o reference/pinned/CommandSystem.cpp
curl -fsSL "https://raw.githubusercontent.com/advancedfx/advancedfx/$commit/shared/CommandSystem.h" -o reference/pinned/CommandSystem.h
python3 ci/generate-original-harness.py \
  reference/pinned/CommandSystem.cpp reference/pinned/CommandSystem.h \
  reference/generated_original.cpp reference/generated_manifest.json
g++ -std=c++17 -O0 -Wall -Wextra -pedantic reference/generated_original.cpp -o reference_trace_cpp
CARGO_INCREMENTAL=0 cargo build --offline --example reference_trace
./reference_trace_cpp > reference_trace_cpp.txt
./target/debug/examples/reference_trace > reference_trace_rust.txt
diff -u reference_trace_cpp.txt reference_trace_rust.txt
echo "Pinned original C++/Rust scheduler traces match"
