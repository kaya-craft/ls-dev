#!/usr/bin/env bash
# Builds a signed and notarized universal LsDev.dmg.
#
# Notarization credentials, one of:
#   APPLE_API_ISSUER, APPLE_API_KEY, APPLE_API_KEY_PATH   (App Store Connect API key)
#   APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID              (Apple ID + app-specific password)
# APPLE_SIGNING_IDENTITY is optional: the first "Developer ID Application"
# identity of the keychain is used otherwise.
set -euo pipefail

cd "$(dirname "$0")/.."

fail() { echo "error: $*" >&2; exit 1; }

if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  APPLE_SIGNING_IDENTITY=$(security find-identity -v -p codesigning \
    | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -n 1)
  [[ -n "$APPLE_SIGNING_IDENTITY" ]] || fail "no \"Developer ID Application\" certificate in the keychain"
  export APPLE_SIGNING_IDENTITY
fi
echo "Signing identity: $APPLE_SIGNING_IDENTITY"

if [[ -n "${APPLE_API_ISSUER:-}${APPLE_API_KEY:-}${APPLE_API_KEY_PATH:-}" ]]; then
  [[ -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_KEY_PATH:-}" ]] \
    || fail "set APPLE_API_ISSUER, APPLE_API_KEY and APPLE_API_KEY_PATH together"
  [[ -f "$APPLE_API_KEY_PATH" ]] || fail "APPLE_API_KEY_PATH does not exist: $APPLE_API_KEY_PATH"
  echo "Notarization: App Store Connect API key $APPLE_API_KEY"
elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  echo "Notarization: Apple ID $APPLE_ID"
else
  fail "no notarization credentials (see the header of this script)"
fi

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target list --installed | grep -qx "$target" || rustup target add "$target"
done

pnpm install --frozen-lockfile
pnpm tauri build --target universal-apple-darwin

bundle=src-tauri/target/universal-apple-darwin/release/bundle
app="$bundle/macos/LsDev.app"
dmg=$(ls -t "$bundle"/dmg/*.dmg | head -n 1)

codesign --verify --deep --strict "$app"
spctl --assess --type execute "$app"
xcrun stapler validate "$app"
spctl --assess --type open --context context:primary-signature "$dmg"

echo "Done: $dmg"
