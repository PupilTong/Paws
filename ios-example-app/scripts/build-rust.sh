#!/bin/sh
set -e

# Xcode strips the user PATH; ensure cargo and common Homebrew installs are visible.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if [ -z "$DEVELOPER_DIR" ]; then
    DEVELOPER_DIR=$(xcode-select -p 2>/dev/null || echo /Applications/Xcode.app/Contents/Developer)
    export DEVELOPER_DIR
fi

if [ "$PLATFORM_NAME" = "iphonesimulator" ]; then
    SDKROOT=$(xcrun --sdk iphonesimulator --show-sdk-path)
    RUST_TARGET="aarch64-apple-ios-sim"
else
    SDKROOT=$(xcrun --sdk iphoneos --show-sdk-path)
    RUST_TARGET="aarch64-apple-ios"
fi
export SDKROOT

if [ "$CONFIGURATION" = "Release" ]; then
    CARGO_FLAGS="--release"
    CARGO_PROFILE="release"
else
    CARGO_FLAGS="--profile ios-dev"
    CARGO_PROFILE="ios-dev"
fi

WORKSPACE_ROOT="$SRCROOT/.."
unset CARGO_TARGET_DIR
CARGO_TARGET_DIR="$WORKSPACE_ROOT/target"
mkdir -p \
    "$CARGO_TARGET_DIR/aarch64-apple-ios-sim/$CARGO_PROFILE" \
    "$CARGO_TARGET_DIR/aarch64-apple-ios/$CARGO_PROFILE"

STAMP_DIR="$CARGO_TARGET_DIR/xcode"
mkdir -p "$STAMP_DIR"
FINGERPRINT_FILE="$STAMP_DIR/${CONFIGURATION}-${PLATFORM_NAME}-${RUST_TARGET}.fingerprint"
BUILD_STAMP="$STAMP_DIR/${CONFIGURATION}-${PLATFORM_NAME}-${RUST_TARGET}.stamp"
XCODE_BUILD_STAMP="$STAMP_DIR/${CONFIGURATION}-${PLATFORM_NAME}-${CURRENT_ARCH:-$RUST_TARGET}.stamp"
STATIC_LIB="$CARGO_TARGET_DIR/$RUST_TARGET/$CARGO_PROFILE/libios_renderer_backend.a"
WASM_STAGE="$CARGO_TARGET_DIR/wasm-examples"
WASM_DEST="$SRCROOT/PawsExample/Examples"

fingerprint_inputs() {
    cd "$WORKSPACE_ROOT"
    {
        printf '%s\n' "$CONFIGURATION" "$PLATFORM_NAME" "$RUST_TARGET" "$CARGO_PROFILE"
        for path in \
            Cargo.toml Cargo.lock \
            engine engine-ua-stylesheet wasmtime-engine \
            ios-renderer-backend/Cargo.toml ios-renderer-backend/build.rs \
            ios-renderer-backend/cbindgen.toml ios-renderer-backend/src \
            rust-wasm-binding paws-style-ir view-macros wit examples \
            yew/packages/yew/Cargo.toml yew/packages/yew/src \
            yew/packages/yew-macro/Cargo.toml yew/packages/yew-macro/src \
            ios-example-app/scripts/build-rust.sh
        do
            if [ -f "$path" ]; then
                shasum -a 256 "$path"
            elif [ -d "$path" ]; then
                find "$path" -type f \
                    ! -path '*/target/*' \
                    ! -name 'ios_renderer_backend.h' \
                    -print | LC_ALL=C sort | while IFS= read -r file; do
                        shasum -a 256 "$file"
                    done
            fi
        done
    } | shasum -a 256 | awk '{print $1}'
}

copy_wasm_examples() {
    mkdir -p "$WASM_DEST"

    for staged in "$WASM_STAGE"/*.wasm; do
        [ -e "$staged" ] || continue
        dest="$WASM_DEST/$(basename "$staged")"
        if [ ! -f "$dest" ] || ! cmp -s "$staged" "$dest"; then
            cp "$staged" "$dest"
        fi
    done

    for dest in "$WASM_DEST"/*.wasm; do
        [ -e "$dest" ] || continue
        staged="$WASM_STAGE/$(basename "$dest")"
        if [ ! -f "$staged" ]; then
            rm -f "$dest"
        fi
    done
}

CURRENT_FINGERPRINT=$(fingerprint_inputs)
if [ -f "$FINGERPRINT_FILE" ] &&
   [ "$(cat "$FINGERPRINT_FILE")" = "$CURRENT_FINGERPRINT" ] &&
   [ -f "$STATIC_LIB" ] &&
   [ -d "$WASM_STAGE" ] &&
   [ "$(find "$WASM_STAGE" -maxdepth 1 -name '*.wasm' | wc -l | tr -d ' ')" != "0" ]; then
    copy_wasm_examples
    touch "$BUILD_STAMP" "$XCODE_BUILD_STAMP"
    echo "Rust artifacts are up to date for $CONFIGURATION/$PLATFORM_NAME"
    exit 0
fi

cd "$WORKSPACE_ROOT"

# iOS static library. Debug uses an optimized custom profile so simulator
# startup reflects runtime costs rather than unoptimized Rust codegen.
cargo build --target "$RUST_TARGET" -p ios-renderer-backend $CARGO_FLAGS
echo "$CARGO_TARGET_DIR/$RUST_TARGET/$CARGO_PROFILE" > "$STAMP_DIR/lib-path.txt"

# Build every WASM example via the paws-examples crate's build.rs. Always use
# release for guest WASM because Yew debug builds are too large for app startup
# analysis and normal example usage.
cargo build -p paws-examples --release

if [ ! -d "$WASM_STAGE" ]; then
    echo "error: wasm staging dir $WASM_STAGE does not exist (paws-examples build.rs did not run?)" >&2
    exit 1
fi

copy_wasm_examples
printf '%s\n' "$CURRENT_FINGERPRINT" > "$FINGERPRINT_FILE"
touch "$BUILD_STAMP" "$XCODE_BUILD_STAMP"

COUNT=$(find "$WASM_DEST" -maxdepth 1 -name '*.wasm' | wc -l | tr -d ' ')
echo "Prepared $COUNT wasm examples in $WASM_DEST"
