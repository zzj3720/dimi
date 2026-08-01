#!/usr/bin/env bash
#
# Dimi installer — downloads the latest release from GitHub and installs it
# into ~/.dimi/bin. Usage:
#
#   curl -fsSL https://github.com/zzj3720/dimi/releases/latest/download/install.sh | bash
#
set -euo pipefail

DIMI_HOME="${DIMI_HOME:-$HOME/.dimi}"
RELEASE_BASE="https://github.com/zzj3720/dimi/releases/latest/download"
MANIFEST_URL="$RELEASE_BASE/manifest.json"

case "$(uname -s)" in
  Darwin) os="darwin" ;;
  Linux) os="linux" ;;
  MINGW*|MSYS*|CYGWIN*)
    echo "Dimi on Windows: use PowerShell instead — irm $RELEASE_BASE/install.ps1 | iex" >&2
    exit 1
    ;;
  *)
    echo "Unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  arm64|aarch64) arch="arm64" ;;
  x86_64|amd64) arch="x64" ;;
  *)
    echo "Unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

target="$os-$arch"

echo "Downloading manifest: $MANIFEST_URL"
manifest="$(mktemp)"
curl -fsSL "$MANIFEST_URL" -o "$manifest"

if ! command -v node >/dev/null 2>&1; then
  echo "Error: manifest parsing needs node (or python3)." >&2
  exit 1
fi

entry="$(node -e '
  const fs = require("fs");
  const manifest = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  const target = process.argv[2];
  const entry = manifest.platforms && manifest.platforms[target];
  if (!entry) { console.error("no build for " + target + " in manifest"); process.exit(1); }
  process.stdout.write(JSON.stringify(entry));
' "$manifest" "$target")"

filename="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).filename)' "$entry")"
checksum="$(node -e 'process.stdout.write(JSON.parse(process.argv[1]).checksum)' "$entry")"

zip_url="$RELEASE_BASE/$filename"
zip_path="$(mktemp -d)/$filename"
echo "Downloading: $zip_url"
curl -fsSL "$zip_url" -o "$zip_path"

echo "Verifying sha256..."
actual="$(shasum -a 256 "$zip_path" | awk '{print $1}')"
if [ "$actual" != "$checksum" ]; then
  echo "Checksum mismatch: expected $checksum, got $actual" >&2
  exit 1
fi

bin_dir="$DIMI_HOME/bin"
mkdir -p "$bin_dir"
echo "Extracting to $bin_dir..."
tar -xzf "$zip_path" -C "$bin_dir" 2>/dev/null || unzip -oq "$zip_path" -d "$bin_dir"
chmod +x "$bin_dir/dimi" 2>/dev/null || true

rm -f "$manifest" "$zip_path"

echo
echo "Installed Dimi to $bin_dir/dimi"
echo "Add it to your PATH:"
echo "  export PATH=\"\$HOME/.dimi/bin:\$PATH\""
