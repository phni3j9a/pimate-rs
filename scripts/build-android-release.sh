#!/usr/bin/env bash
set +x
set -euo pipefail
cd "$(dirname "$0")/.."
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
signing="$HOME/.config/pimate-release"
export PIMATE_SIGNING_PROPERTIES="$signing/signing.properties"
for file in "$PIMATE_SIGNING_PROPERTIES" "$signing/pimate-release.jks"; do
  [[ -f "$file" && -r "$file" ]] || { echo 'Restore the existing release key and properties; no key was created.' >&2; exit 1; }
done
# preBuild runs build-android.sh (cargo ndk --release) before packaging.
(cd apps/android && ./gradlew --no-configuration-cache --no-daemon --max-workers 2 :app:assembleRelease)
apk=apps/android/app/build/outputs/apk/release/app-release.apk
apksigner="$ANDROID_HOME/build-tools/36.0.0/apksigner"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
reference=pimate-0.1.23.apk
gh release download app-v0.1.23 --repo phni3j9a/PiMate --pattern "$reference" --dir "$tmp"
cert() {
  "$apksigner" verify --print-certs "$1" | sed -n 's/^Signer #[0-9]* certificate SHA-256 digest: //p'
}
current=$(cert "$apk")
previous=$(cert "$tmp/$reference")
[[ "$current" =~ ^[0-9a-f]{64}$ && "$current" == "$previous" ]] || { echo 'Release certificate mismatch' >&2; exit 1; }
printf 'Certificate SHA-256 (matches TS 0.1.23): %s\n' "$current"
out="apps/android/dist/pimate-$(<apps/android/version.txt).apk"
mkdir -p apps/android/dist
cp "$apk" "$out"
sha256sum "$out"
