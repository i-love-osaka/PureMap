#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
APP_PATH=${1:-"$PROJECT_ROOT/PureMap.app"}
CONTENTS_PATH="$APP_PATH/Contents"

cd "$PROJECT_ROOT"
cargo build --release --locked

mkdir -p "$CONTENTS_PATH/MacOS"
mkdir -p "$CONTENTS_PATH/Resources"
cp "$PROJECT_ROOT/target/release/puremap" "$CONTENTS_PATH/MacOS/puremap"
cp "$PROJECT_ROOT/packaging/macos/Info.plist" "$CONTENTS_PATH/Info.plist"

# Single source of truth for the version: Cargo.toml. The plist in
# the repo carries a __VERSION__ placeholder; stamp the real value
# into the bundled copy (the About window reads Cargo's version
# too, so the two can never drift apart again).
VERSION=$(sed -n '/^\[package\]/,/^\[/p' "$PROJECT_ROOT/Cargo.toml" \
    | sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
    | head -n 1)
if [ -z "$VERSION" ]; then
    printf 'Could not read version from Cargo.toml\n' >&2
    exit 1
fi
sed -i '' "s/__VERSION__/$VERSION/g" "$CONTENTS_PATH/Info.plist" 2>/dev/null \
    || sed -i "s/__VERSION__/$VERSION/g" "$CONTENTS_PATH/Info.plist"

# App icon: prefer the Icon Composer asset. actool compiles
# packaging/macos/Icon.icon into Assets.car + Icon.icns, which the
# current macOS renders natively (proper squircle, light/dark
# appearances, no fallback plate). Without actool (no Xcode), fall
# back to a classic .icns under the same name so the build still
# works — the icon then renders the classic way.
ICON_SRC="$PROJECT_ROOT/packaging/macos/Icon.icon"
ACTOOL=""
if command -v xcrun >/dev/null 2>&1; then
    ACTOOL=$(xcrun --find actool 2>/dev/null || true)
fi
if [ -n "$ACTOOL" ] && [ -d "$ICON_SRC" ]; then
    # Keep actool's output in icon-build.log next to the project for
    # diagnosis, and never let an actool failure break the build:
    # fall back to the classic icon so the app always ships with one.
    if "$ACTOOL" "$ICON_SRC" \
        --compile "$CONTENTS_PATH/Resources" \
        --app-icon Icon \
        --enable-on-demand-resources NO \
        --development-region en \
        --target-device mac \
        --minimum-deployment-target 26.0 \
        --platform macosx \
        --output-partial-info-plist "${TMPDIR:-/tmp}/puremap-icon-partial.plist" \
        > "$PROJECT_ROOT/icon-build.log" 2>&1; then
        rm -f "$CONTENTS_PATH/Resources/PureMap.icns"
    else
        printf 'actool failed; falling back to classic icns (see icon-build.log)\n' >&2
        rm -f "$CONTENTS_PATH/Resources/Assets.car"
        iconutil -c icns "$PROJECT_ROOT/packaging/macos/PureMap.iconset" \
            -o "$CONTENTS_PATH/Resources/Icon.icns"
    fi
else
    rm -f "$CONTENTS_PATH/Resources/Assets.car"
    iconutil -c icns "$PROJECT_ROOT/packaging/macos/PureMap.iconset" \
        -o "$CONTENTS_PATH/Resources/Icon.icns"
fi

chmod 755 "$CONTENTS_PATH/MacOS/puremap"

if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$APP_PATH"
fi

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
if [ -x "$LSREGISTER" ]; then
    "$LSREGISTER" -f "$APP_PATH"
fi

printf 'Built macOS app: %s\n' "$APP_PATH"
