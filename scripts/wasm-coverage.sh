#!/usr/bin/env bash
# Post-processes WASM guest profraw files into lcov for Codecov.
#
# Prerequisites:
#   - `llvm-tools-preview` rustup component installed
#   - Profraw files written to target/wasm-coverage/ by E2E tests
#   - Instrumented .wasm binaries produced by the coverage build
#
# Usage:
#   PAWS_WASM_COVERAGE=1 cargo test -p wasmtime-engine --test e2e_examples --features wasm-coverage
#   bash scripts/wasm-coverage.sh
#
# Output: target/wasm-coverage/guest-lcov.info

set -euo pipefail
# Non-matching globs expand to an empty list instead of the literal pattern,
# so the `.profraw` length check below behaves correctly.
shopt -s nullglob

COVERAGE_DIR="target/wasm-coverage"

# ---------------------------------------------------------------------------
# Resolve LLVM tools from the active Rust toolchain's sysroot
# ---------------------------------------------------------------------------
SYSROOT=$(rustc --print sysroot)
HOST=$(rustc -vV | grep host | cut -d' ' -f2)
LLVM_BIN="${SYSROOT}/lib/rustlib/${HOST}/bin"

PROFDATA="${LLVM_BIN}/llvm-profdata"
LLVM_COV="${LLVM_BIN}/llvm-cov"

for tool in "$PROFDATA" "$LLVM_COV"; do
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
# 2. Collect instrumented .wasm binaries
# ---------------------------------------------------------------------------
# `-Cinstrument-coverage` embeds `__llvm_covmap` directly into the linked
# .wasm artifact, and `llvm-cov export` can read it straight from there.
# No separate object-file extraction is needed.
#
# Examples have their own nested target/ trees (each crate is its own
# mini-workspace under examples/), so we search under examples/ and yew/.
WASM_ARGS=()
WASM_COUNT=0
while IFS= read -r -d '' wasm_file; do
    # Skip deps/ copies — use only the final linked artifact per crate.
    case "$wasm_file" in
        */deps/*) continue ;;
    esac
    WASM_ARGS+=("-object=${wasm_file}")
    WASM_COUNT=$((WASM_COUNT + 1))
done < <(find examples/ yew/ -name "*.wasm" -path "*/release/*" -print0 2>/dev/null)

if [[ $WASM_COUNT -eq 0 ]]; then
    echo "error: no .wasm files found under examples/ or yew/" >&2
    echo "  Ensure guest code was built with PAWS_WASM_COVERAGE=1" >&2
    exit 1
fi
echo "Found ${WASM_COUNT} instrumented .wasm binary(s)."

# ---------------------------------------------------------------------------
# 3. Generate lcov from profdata + .wasm binaries
# ---------------------------------------------------------------------------
# Some instrumented wasm artifacts can emit malformed coverage records
# (e.g. "function name is empty") when yew's aggressive release profile
# — opt-level="z", codegen-units=1 — merges generic monomorphisations
# such that LLVM strips their symbol names. Feeding every object in a
# single llvm-cov call fails fast on the first bad one, so we process
# each wasm separately and skip individual failures, preserving
# coverage for the rest.
echo "Generating guest-lcov.info..."
: > "${COVERAGE_DIR}/guest-lcov.info"
SKIPPED=0
for wasm_arg in "${WASM_ARGS[@]}"; do
    wasm_path="${wasm_arg#-object=}"
    if ! "$LLVM_COV" export --format=lcov \
        --instr-profile="${COVERAGE_DIR}/merged.profdata" \
        "-object=${wasm_path}" \
        >> "${COVERAGE_DIR}/guest-lcov.info" 2>/dev/null
    then
        echo "warning: skipping ${wasm_path} (llvm-cov export failed)" >&2
        SKIPPED=$((SKIPPED + 1))
    fi
done

LINES=$(wc -l < "${COVERAGE_DIR}/guest-lcov.info")
echo "Done: ${COVERAGE_DIR}/guest-lcov.info (${LINES} lines, ${SKIPPED} wasm skipped)"
