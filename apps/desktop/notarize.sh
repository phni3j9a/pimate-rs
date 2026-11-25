#!/usr/bin/env bash
# Run only on an ephemeral macOS runner in the protected release environment.
set -euo pipefail
if [[ -z $P12 && -z $P12_PASSWORD && -z $ASC_KEY && -z $ASC_KEY_ID && -z $ASC_ISSUER ]]; then
  echo 'No Apple credentials: keeping ad-hoc signatures.'
  echo 'signing=ad-hoc・未公証' >> "$GITHUB_OUTPUT"; exit 0
fi
: "${P12:?}" "${P12_PASSWORD:?}" "${ASC_KEY:?}" "${ASC_KEY_ID:?}" "${ASC_ISSUER:?}"
umask 077
tmp=$(mktemp -d); chain="$tmp/sign.keychain-db"; pass=$(openssl rand -hex 24)
trap 'security delete-keychain "$chain" || :; rm -rf "$tmp"' EXIT
printf %s "$P12" | base64 -D > "$tmp/id.p12"
printf %s "$ASC_KEY" | base64 -D > "$tmp/key.p8"
security create-keychain -p "$pass" "$chain"
security unlock-keychain -p "$pass" "$chain"
security import "$tmp/id.p12" -k "$chain" -P "$P12_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$pass" "$chain" >/dev/null
identity=$(security find-identity -v -p codesigning "$chain" | awk '/"Developer ID Application:/ {print $2}')
[[ $identity =~ ^[0-9A-Fa-f]{40}$ ]]
for zip in "$@"; do
  app="$tmp/PiMate Desktop.app"; ditto -x -k "$zip" "$tmp"
  codesign --force --options runtime --timestamp --keychain "$chain" --sign "$identity" "$app"
  codesign --verify --strict "$app"
  ditto -c -k --keepParent "$app" "$tmp/submit.zip"
  xcrun notarytool submit "$tmp/submit.zip" --key "$tmp/key.p8" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER" --wait --output-format json | jq -e '.status == "Accepted"'
  xcrun stapler staple "$app"; xcrun stapler validate "$app"
  spctl --assess --type execute "$app"
  rm "$zip"; ditto -c -k --keepParent "$app" "$zip"
  rm -rf "$app" "$tmp/submit.zip"
done
echo 'signing=Developer ID 署名・公証済み' >> "$GITHUB_OUTPUT"
