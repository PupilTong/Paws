#!/usr/bin/env bash
# Post-processes WASM guest profraw files into lcov for Codecov.
#
# Prerequisites:
#   - `llvm-tools-preview` rustup component installed
#   - Profraw files written to target/wasm-coverage/ by E2E tests
#   - LLVM IR (.ll) files emitted during the coverage build
#
# Usage:
#   PAWS_WASM_COVERAGE=1 cargo test -p wasmtime-engine --test e2e_examples --features wasm-coverage
#   bash scripts/wasm-coverage.sh
#
# Output: target/wasm-coverage/guest-lcov.info

set -euo pipefail

COVERAGE_DIR="target/wasm-coverage"

# ---------------------------------------------------------------------------
# Resolve LLVM tools from the active Rust toolchain's sysroot
# ---------------------------------------------------------------------------
SYSROOT=$(rustc --print sysroot)
HOST=$(rustc -vV | grep host | cut -d' ' -f2)
LLVM_BIN="${SYSROOT}/lib/rustlib/${HOST}/bin"

PROFDATA="${LLVM_BIN}/llvm-profdata"
LLVM_COV="${LLVM_BIN}/llvm-cov"
LLC="${LLVM_BIN}/llc"

for tool in "$PROFDATA" "$LLVM_COV" "$LLC"; do
    if [[ ! -x "$tool" ]]; then
        echo "error: $tool not found — install the llvm-tools-preview component:" >&2
        echo "  rustup component add llvm-tools-preview" >&2
        exit 1
    fi
done

# ---------------------------------------------------------------------------
# 1. Merge all profraw files into a single profdata
# ---------------------------------------------------------------------------
PROFRAW_FILES=("${COVERAGE_DIR}"/*.profraw)
if [[ ${#PROFRAW_FILES[@]} -eq 0 ]]; then
    echo "error: no .profraw files found in ${COVERAGE_DIR}/" >&2
    echo "  Run tests first: PAWS_WASM_COVERAGE=1 cargo test -p wasmtime-engine --test e2e_examples --features wasm-coverage" >&2
    exit 1
fi

echo "Merging ${#PROFRAW_FILES[@]} profraw file(s)..."
"$PROFDATA" merge -sparse "${PROFRAW_FILES[@]}" -o "${COVERAGE_DIR}/merged.profdata"

# ---------------------------------------------------------------------------
# 2. Compile LLVM IR (.ll) files to object files (.o) with __llvm_covmap
# ---------------------------------------------------------------------------
# The .ll files are emitted by --emit=llvm-ir during the coverage build.
# They live in the deps/ subdirectory of each example's target directory.
mkdir -p "${COVERAGE_DIR}/objects"

LL_COUNT=0
for ll_file in $(find examples/ yew/ -name "*.ll" -path "*/deps/*" 2>/dev/null); do
    obj_name=$(basename "${ll_file%.ll}.o")
    "$LLC" -filetype=obj "$ll_file" -o "${COVERAGE_DIR}/objects/${obj_name}" 2>/dev/null || {
        echo "  warning: failed to compile ${ll_file}, skipping" >&2
        continue
    }
    LL_COUNT=$((LL_COUNT + 1))
done

if [[ $LL_COUNT -eq 0 ]]; then
    echo "error: no .ll files found under examples/ or yew/" >&2
    echo "  Ensure guest code was built with PAWS_WASM_COVERAGE=1 (emits LLVM IR)" >&2
    exit 1
fi
echo "Compiled ${LL_COUNT} LLVM IR file(s) to object files."

# ---------------------------------------------------------------------------
# 3. Generate lcov from profdata + object files
# ---------------------------------------------------------------------------
OBJECT_ARGS=()
for obj_file in "${COVERAGE_DIR}"/objects/*.o; do
    OBJECT_ARGS+=("-object=${obj_file}")
done

echo "Generating guest-lcov.info..."
"$LLVM_COV" export --format=lcov \
    --instr-profile="${COVERAGE_DIR}/merged.profdata" \
    "${OBJECT_ARGS[@]}" \
    > "${COVERAGE_DIR}/guest-lcov.info"

LINES=$(wc -l < "${COVERAGE_DIR}/guest-lcov.info")
echo "Done: ${COVERAGE_DIR}/guest-lcov.info (${LINES} lines)"
