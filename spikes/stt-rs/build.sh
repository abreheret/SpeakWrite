#!/usr/bin/env sh
# Build the Kyutai STT spike on Linux or macOS.
# macOS -> Metal, Linux with nvcc -> CUDA, otherwise CPU. Extra args go to cargo.
set -eu
cd "$(dirname "$0")"

# CMake 4 dropped support for cmake_minimum_required < 3.5 (old opus in audiopus_sys).
export CMAKE_POLICY_VERSION_MINIMUM="${CMAKE_POLICY_VERSION_MINIMUM:-3.5}"

case "$(uname -s)" in
    Darwin) features="metal" ;;
    *)
        if command -v nvcc >/dev/null 2>&1; then
            features="cuda"
        else
            features=""
        fi
        ;;
esac

if [ -n "$features" ]; then
    echo "Building with --features $features"
    exec cargo build --release --features "$features" "$@"
else
    echo "nvcc not found: building CPU-only"
    exec cargo build --release "$@"
fi
