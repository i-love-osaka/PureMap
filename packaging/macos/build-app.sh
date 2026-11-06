#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
APP_PATH=${1:-"$PROJECT_ROOT/PureMap.app"}
CONTENTS_PATH="$APP_PATH/Contents"

cd "$PROJECT_ROOT"
cargo build --release

mkdir -p "$CONTENTS_PATH/MacOS"
mkdir -p "$CONTENTS_PATH/Resources"
cp "$PROJECT_ROOT/target/release/puremap" "$CONTENTS_PATH/MacOS/puremap"
cp "$PROJECT_ROOT/packaging/macos/Info.plist" "$CONTENTS_PATH/Info.plist"
iconutil -c icns "$PROJECT_ROOT/packaging/macos/PureMap.iconset" \
    -o "$CONTENTS_PATH/Resources/PureMap.icns"
chmod 755 "$CONTENTS_PATH/MacOS/puremap"

if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$APP_PATH"
fi

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
if [ -x "$LSREGISTER" ]; then
    "$LSREGISTER" -f "$APP_PATH"
fi

printf 'Built macOS app: %s\n' "$APP_PATH"
