#!/bin/sh
# Builds Tonefold for macOS (Apple Silicon and Intel in one universal build) and stages
# dist/Tonefold-<version>-macOS.dmg: Tonefold.app, plus the optional CLAP and VST3 plugins.
#
#   scripts/package-macos.sh               build everything, then package
#   SKIP_BUILD=1 scripts/package-macos.sh  package what is already built
#
# The app carries the composer (installed with npm on first use, as the Windows installer does)
# and the GeneralUser GS soundfont. Set TONEFOLD_SIGN_IDENTITY to sign with a Developer ID;
# otherwise the app is ad-hoc signed.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)".*/\1/p' Cargo.toml | tr -d '\r' | head -n 1)
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }
echo "Packaging Tonefold $version for macOS"

targets="aarch64-apple-darwin x86_64-apple-darwin"
if [ "${SKIP_BUILD:-}" != 1 ]; then
    echo "  building the app and CLI..."
    for t in $targets; do
        cargo build --release --target "$t" -p tonefold-plugin --features standalone --bin tonefold-standalone
        cargo build --release --target "$t" -p tonefold-cli
    done
    echo "  building the plugins..."
    cargo xtask bundle-universal tonefold-plugin --release
    echo "  building the composer..."
    (cd agent && npm ci --no-audit --no-fund && npm run build)
fi

out="dist/macos"
stage="$out/Tonefold $version"
rm -rf "$out"
mkdir -p "$stage"

# --- Tonefold.app ------------------------------------------------------------------------------
app="$stage/Tonefold.app"
res="$app/Contents/Resources"
mkdir -p "$app/Contents/MacOS" "$res/bin"
lipo -create -output "$app/Contents/MacOS/Tonefold" \
    target/aarch64-apple-darwin/release/tonefold-standalone target/x86_64-apple-darwin/release/tonefold-standalone
lipo -create -output "$res/bin/tonefold-cli" \
    target/aarch64-apple-darwin/release/tonefold-cli target/x86_64-apple-darwin/release/tonefold-cli
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp packaging/macos/Tonefold.icns "$res/Tonefold.icns"

# The composer without node_modules: the app runs npm ci into the data folder on first use.
mkdir -p "$res/agent"
for item in dist prompts skills reference package.json package-lock.json; do
    cp -R "agent/$item" "$res/agent/"
done
cp -R skill "$res/skill"

# Built-in sounds. Its author asks that projects ship their own copy rather than link to his files.
sf="${TONEFOLD_SOUNDFONT_SRC:-target/GeneralUser-GS.sf2}"
if [ ! -f "$sf" ]; then
    echo "  fetching the GeneralUser GS soundfont..."
    curl -fsSL -o "$sf" https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/main/GeneralUser-GS.sf2
    curl -fsSL -o target/GeneralUser-GS-LICENSE.txt https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/main/documentation/LICENSE.txt
fi
mkdir -p "$res/soundfont"
cp "$sf" "$res/soundfont/GeneralUser-GS.sf2"
[ -f target/GeneralUser-GS-LICENSE.txt ] && cp target/GeneralUser-GS-LICENSE.txt "$res/soundfont/LICENSE.txt"
cp LICENSE THIRD_PARTY_NOTICES.md "$res/"

if [ -n "${TONEFOLD_SIGN_IDENTITY:-}" ]; then
    codesign --force --deep --options runtime --timestamp --sign "$TONEFOLD_SIGN_IDENTITY" "$app"
else
    codesign --force --deep --sign - "$app"
fi

# --- optional DAW plugins ----------------------------------------------------------------------
mkdir -p "$stage/DAW plugins"
cp -R target/bundled/Tonefold.clap target/bundled/Tonefold.vst3 "$stage/DAW plugins/"
for p in "$stage/DAW plugins/Tonefold.clap" "$stage/DAW plugins/Tonefold.vst3"; do
    codesign --force --deep --sign "${TONEFOLD_SIGN_IDENTITY:--}" "$p"
done
cp packaging/macos/README.txt "$stage/Read me first.txt"
ln -s /Applications "$stage/Applications"

dmg="dist/Tonefold-$version-macOS.dmg"
rm -f "$dmg"
hdiutil create -quiet -volname "Tonefold $version" -srcfolder "$stage" -fs HFS+ -format UDZO "$dmg"
echo "Created $dmg"
