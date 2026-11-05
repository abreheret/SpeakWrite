#!/usr/bin/env sh
# Release build of SpeakWrite on Linux or macOS.
# Dictation uses Metal on macOS, CUDA on Linux when nvcc is available, the CPU otherwise.
# Extra arguments are passed to cargo.
set -eu
cd "$(dirname "$0")/.."

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
    exec cargo build --release -p speakwrite --features "$features" "$@"
else
    echo "nvcc not found: dictation will run on the CPU"
    exec cargo build --release -p speakwrite "$@"
fi
