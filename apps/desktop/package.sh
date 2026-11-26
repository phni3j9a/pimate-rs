#!/usr/bin/env bash
# Package a native, locked release build. Run from the repository root.
set -euo pipefail
version=$(cargo metadata --no-deps --locked --format-version 1 | jq -er '.packages[] | select(.name == "pimate-desktop").version')
arch=$(uname -m); icons=docs/design/icon-kit/desktop
stage=$(mktemp -d); trap 'rm -rf "$stage"' EXIT
bin=${CARGO_TARGET_DIR:-target}/release/pimate-desktop
mkdir -p dist "$stage/notices"
if [[ $# == 0 ]]; then
  cargo about generate --locked --fail --manifest-path apps/desktop/Cargo.toml -c apps/desktop/about.toml packages/pimate/notices/licenses.hbs -o "$stage/notices/crates.txt"
else
  cp "$1" "$stage/notices/crates.txt"
fi
cp LICENSE apps/ios/App/Lucide-LICENSE.txt "$stage/notices/"
cp "$(rustc --print sysroot)/share/doc/rust/COPYRIGHT-library.html" "$stage/notices/"
case $(uname -s) in
Darwin)
  app="$stage/PiMate Desktop.app"; contents="$app/Contents"; p="$contents/Info.plist"
  mkdir -p "$contents/MacOS" "$contents/Resources"
  install -m755 "$bin" "$contents/MacOS/pimate-desktop"
  cp "$icons/macos/pimate-light.icns" "$contents/Resources/PiMate.icns"
  mv "$stage/notices" "$contents/Resources/"
  cp apps/desktop/Info.plist "$p"
  plutil -replace CFBundleName -string 'PiMate Desktop' "$p"
  plutil -replace CFBundleIdentifier -string dev.pimate.desktop "$p"
  plutil -replace CFBundleVersion -string "$version" "$p"
  plutil -insert CFBundleShortVersionString -string "$version" "$p"
  plutil -insert CFBundleIconFile -string PiMate.icns "$p"
  lipo "$contents/MacOS/pimate-desktop" -verify_arch "$arch"
  codesign --force --sign - "$app"
  codesign --verify --strict "$app"
  ditto -c -k --keepParent "$app" "dist/PiMate-Desktop-$version-macos-$arch.zip" ;;
Linux)
  name="PiMate-Desktop-$version-linux-$arch"; tree="$stage/$name"
  mkdir -p "$tree/bin" "$tree/share/applications" "$tree/share/icons/hicolor/256x256/apps"
  install -m755 "$bin" "$tree/bin/pimate-desktop"
  cp "$icons/png/pimate-light-256.png" "$tree/share/icons/hicolor/256x256/apps/pimate-desktop.png"
  mkdir -p "$tree/share/doc/pimate-desktop"
  mv "$stage/notices" "$tree/share/doc/pimate-desktop/"
  cp apps/desktop/pimate-desktop.desktop "$tree/share/applications/"
  tar -C "$stage" -czf "dist/$name.tar.gz" "$name" ;;
*) exit 2 ;;
esac
