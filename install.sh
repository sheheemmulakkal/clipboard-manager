#!/usr/bin/env bash
set -euo pipefail

REPO="sheheemmulakkal/clipboard-manager"
APP="clipboard-manager"

# ── Colors ───────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

info()    { echo -e "${BLUE}  ➜${NC}  $*"; }
success() { echo -e "${GREEN}  ✓${NC}  $*"; }
error()   { echo -e "${RED}  ✗${NC}  $*" >&2; exit 1; }
header()  { echo -e "\n${BOLD}$*${NC}\n"; }

# ── Checks ───────────────────────────────────────────
header "Clipboard Manager Installer"

# Must be run with bash (not sh)
if [ -z "${BASH_VERSION:-}" ]; then
  error "Please run with bash, not sh."
fi

# Must be Debian/Ubuntu
if ! command -v apt-get &>/dev/null; then
  error "This installer only supports Ubuntu/Debian (apt not found)."
fi

# Detect architecture
ARCH=$(dpkg --print-architecture)
if [[ "$ARCH" != "amd64" && "$ARCH" != "arm64" ]]; then
  error "Unsupported architecture: $ARCH. Only amd64 and arm64 are supported."
fi

# Must have curl or wget
# Time out and retry instead of hanging on a stalled connection.
if command -v curl &>/dev/null; then
  CURL_OPTS="--connect-timeout 15 --retry 3 --retry-delay 2 --speed-limit 1024 --speed-time 30"
  DOWNLOAD="curl -fsSL $CURL_OPTS"
  DOWNLOAD_FILE="curl -fSL $CURL_OPTS -o"
elif command -v wget &>/dev/null; then
  DOWNLOAD="wget -qO- --timeout=15 --tries=3"
  DOWNLOAD_FILE="wget -q --timeout=15 --tries=3 -O"
else
  error "curl or wget is required. Install with: sudo apt install curl"
fi

# ── Find latest release ──────────────────────────────
info "Finding latest release..."

RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"
RELEASE_JSON=$($DOWNLOAD "$RELEASE_URL") || error "Could not reach GitHub API."

VERSION=$(echo "$RELEASE_JSON" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
if [[ -z "$VERSION" ]]; then
  error "Could not determine latest version. Is the repo public?"
fi

DEB_URL=$(echo "$RELEASE_JSON" | grep '"browser_download_url"' | grep '\.deb"' | head -1 | cut -d'"' -f4)
if [[ -z "$DEB_URL" ]]; then
  error "No .deb file found in release ${VERSION}."
fi

success "Latest version: ${VERSION}"

# ── Download ─────────────────────────────────────────
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

DEB_FILE="${TMP_DIR}/${APP}.deb"

info "Downloading ${APP} ${VERSION}..."
$DOWNLOAD_FILE "$DEB_FILE" "$DEB_URL" \
  || error "Download failed (network or GitHub problem). Try again in a minute, or download the .deb from https://github.com/${REPO}/releases/latest"
chmod 644 "$DEB_FILE"
success "Downloaded."

# ── Install ──────────────────────────────────────────
info "Installing (sudo required)..."
sudo apt-get install -y "$DEB_FILE" || error "Installation failed."

# ── Start now ────────────────────────────────────────
# The script is still running as the current user here (only the apt-get call
# above used sudo), so we can launch the app directly without needing $DISPLAY
# to survive through sudo.
# The package's post-install script usually started it already.
if pgrep -u "$(id -u)" -f '^(/usr/bin/)?clipboard-manager( |$)' >/dev/null; then
  success "clipboard-manager is running."
elif command -v clipboard-manager &>/dev/null; then
  nohup clipboard-manager &>/dev/null &
  disown
  success "Started clipboard-manager in background."
fi

# ── Done ─────────────────────────────────────────────
echo ""
success "Clipboard Manager ${VERSION} installed!"
echo ""
echo -e "  ${BOLD}How to use:${NC}"
echo "    • Already running — press  Ctrl + Alt + C  to open clipboard history."
echo "    • It will also start automatically on every login."
echo ""
echo -e "  ${BOLD}To uninstall:${NC}"
echo "    sudo apt remove clipboard-manager"
echo ""
