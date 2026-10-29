#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
release_dir="${CARGO_TARGET_DIR:-$root/target}/release"
mkdir -p "$release_dir"
clang_bin="${PLANORA_BPF_CLANG:-}"
if [ -z "$clang_bin" ]; then
  clang_bin="$(command -v clang || command -v clang-18)"
fi
headers=""
for candidate in "/usr/include/$(gcc -dumpmachine)" /usr/include/x86_64-linux-gnu /usr/include; do
  if [ -f "$candidate/asm/types.h" ]; then headers="$candidate"; break; fi
done
if [ -z "$headers" ]; then
  echo 'Linux kernel userspace headers (asm/types.h) are required for the BPF build' >&2
  exit 1
fi
"$clang_bin" -O2 -g -target bpf -D__TARGET_ARCH_x86 \
  -I"$headers" \
  -c "$root/packaging/linux/planora-disk-profile.bpf.c" \
  -o "$release_dir/planora-disk-profile.bpf.o"
"${CC:-cc}" -O2 -Wall -Wextra -Werror -std=c11 \
  "$root/packaging/linux/planora-disk-profile.c" \
  -o "$release_dir/planora-disk-profile" -lbpf -lelf -lz
