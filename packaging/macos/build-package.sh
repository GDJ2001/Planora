#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
target="aarch64-apple-darwin"
version="${PLANORA_VERSION:-0.1.1}"
export RUSTUP_TOOLCHAIN=1.95.0
cargo build --manifest-path "$root/Cargo.toml" --release --target "$target" --bins --locked
stage="$root/target/package/macos"
app="$stage/Applications/Planora.app"
rm -rf "$stage"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Library/LaunchDaemons" "$root/dist"
cp "$root/target/$target/release/planora" "$app/Contents/MacOS/planora"
cp "$root/target/$target/release/planora-helper" "$app/Contents/MacOS/planora-helper"
cp "$root/packaging/macos/Info.plist" "$app/Contents/Info.plist"
cp "$root/packaging/macos/org.planora.helper.plist" "$app/Contents/Library/LaunchDaemons/org.planora.helper.plist"
cp "$root/assets/brand/platforms/macos/Planora.icns" "$app/Contents/Resources/Planora.icns"
identity="${PLANORA_SIGN_IDENTITY:--}"
codesign --force --sign "$identity" "$app/Contents/MacOS/planora-helper"
codesign --force --sign "$identity" "$app"
pkg_args=(--root "$stage" --scripts "$root/packaging/macos/scripts" --identifier org.planora.app --version "$version" --install-location /)
if [ -n "${PLANORA_INSTALLER_IDENTITY:-}" ]; then
  pkg_args+=(--sign "$PLANORA_INSTALLER_IDENTITY")
fi
pkgbuild "${pkg_args[@]}" "$root/dist/Planora-${version}-arm64.pkg"
ditto -c -k --sequesterRsrc --keepParent "$app" "$root/dist/Planora-${version}-arm64-app.zip"
if [ -n "${PLANORA_NOTARY_PROFILE:-}" ]; then
  if [ "$identity" = '-' ] || [ -z "${PLANORA_INSTALLER_IDENTITY:-}" ]; then
    echo 'Notarization requires PLANORA_SIGN_IDENTITY and PLANORA_INSTALLER_IDENTITY.' >&2
    exit 1
  fi
  xcrun notarytool submit "$root/dist/Planora-${version}-arm64.pkg" --keychain-profile "$PLANORA_NOTARY_PROFILE" --wait
  xcrun stapler staple "$root/dist/Planora-${version}-arm64.pkg"
fi
