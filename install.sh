#!/bin/sh
# SkillSupport installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/jamesdigid/SkillSupport/main/install.sh | sh
#
# Environment variables:
#   SKILLSUPPORT_INSTALL_DIR   Where to install the binary (default: $HOME/.caps/bin)
#   SKILLSUPPORT_VERSION       Release tag to install (default: latest)
#   SKILLSUPPORT_FROM_SOURCE   Set to "1" to force building from source with cargo
# Backward compatibility:
#   EPISTEM_INSTALL_DIR, EPISTEM_VERSION, and EPISTEM_FROM_SOURCE are still accepted.
set -eu

REPO="jamesdigid/SkillSupport"
PACKAGE_NAME="skillsupport"
BIN_NAME="caps"
INSTALL_DIR="${SKILLSUPPORT_INSTALL_DIR:-${EPISTEM_INSTALL_DIR:-$HOME/.caps/bin}}"
VERSION="${SKILLSUPPORT_VERSION:-${EPISTEM_VERSION:-latest}}"

# ---------------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------------
if [ -t 1 ]; then
  BOLD="$(printf '\033[1m')"
  DIM="$(printf '\033[2m')"
  RED="$(printf '\033[31m')"
  GREEN="$(printf '\033[32m')"
  YELLOW="$(printf '\033[33m')"
  RESET="$(printf '\033[0m')"
else
  BOLD="" DIM="" RED="" GREEN="" YELLOW="" RESET=""
fi

info() { printf '%s==>%s %s\n' "$GREEN" "$RESET" "$1"; }
warn() { printf '%swarning:%s %s\n' "$YELLOW" "$RESET" "$1" >&2; }
error() {
  printf '%serror:%s %s\n' "$RED" "$RESET" "$1" >&2
  exit 1
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    error "required command '$1' not found on PATH"
  fi
}

# ---------------------------------------------------------------------------
# Detect platform -> Rust target triple
# ---------------------------------------------------------------------------
detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux) os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *) error "unsupported operating system: $os" ;;
  esac

  case "$arch" in
    x86_64 | amd64) arch_part="x86_64" ;;
    arm64 | aarch64) arch_part="aarch64" ;;
    *) error "unsupported architecture: $arch" ;;
  esac

  echo "${arch_part}-${os_part}"
}

# ---------------------------------------------------------------------------
# Download helpers
# ---------------------------------------------------------------------------
download() {
  # download <url> <output>
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$2" "$1"
  else
    error "need curl or wget to download files"
  fi
}

fetch() {
  # fetch <url> -> stdout
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO - "$1"
  else
    error "need curl or wget to download files"
  fi
}

# Resolve VERSION="latest" to the most recent release tag. Unlike the
# /releases/latest endpoint, the /releases list includes pre-releases, so this
# works for early 0.0.x pre-release builds too. GitHub returns releases newest
# first, so the first tag_name is what we want.
resolve_version() {
  [ "$VERSION" = "latest" ] || return 0

  api="https://api.github.com/repos/${REPO}/releases?per_page=1"
  tag="$(fetch "$api" 2>/dev/null \
    | grep -m1 '"tag_name"' \
    | sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"

  if [ -n "${tag:-}" ]; then
    VERSION="$tag"
  fi
  return 0
}

asset_url() {
  # asset_url <target>
  filename="${BIN_NAME}-${1}.tar.gz"
  if [ "$VERSION" = "latest" ]; then
    # Fallback if the API could not be reached; only finds stable releases.
    echo "https://github.com/${REPO}/releases/latest/download/${filename}"
  else
    echo "https://github.com/${REPO}/releases/download/${VERSION}/${filename}"
  fi
}

# ---------------------------------------------------------------------------
# Install strategies
# ---------------------------------------------------------------------------
install_from_release() {
  target="$1"
  url="$(asset_url "$target")"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  info "Downloading ${BIN_NAME} (${target})"
  if ! download "$url" "$tmp/${BIN_NAME}.tar.gz" 2>/dev/null; then
    rm -rf "$tmp"
    trap - EXIT
    return 1
  fi

  tar -xzf "$tmp/${BIN_NAME}.tar.gz" -C "$tmp" || {
    rm -rf "$tmp"
    trap - EXIT
    return 1
  }

  if [ ! -f "$tmp/${BIN_NAME}" ]; then
    rm -rf "$tmp"
    trap - EXIT
    return 1
  fi

  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$tmp/${BIN_NAME}" "$INSTALL_DIR/${BIN_NAME}"
  rm -rf "$tmp"
  trap - EXIT
  return 0
}

install_from_source() {
  info "Building ${BIN_NAME} from source with cargo"
  if ! command -v cargo >/dev/null 2>&1; then
    error "no prebuilt binary available and 'cargo' is not installed.
  Install Rust from https://rustup.rs and re-run this script, or install a
  released binary once one is published for your platform."
  fi

  # cargo installs into its own bin dir; point users there afterwards.
  cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
  if [ "$VERSION" = "latest" ]; then
    cargo install --git "https://github.com/${REPO}" --bin "$BIN_NAME" --locked "$PACKAGE_NAME"
  else
    cargo install --git "https://github.com/${REPO}" --tag "$VERSION" --bin "$BIN_NAME" --locked "$PACKAGE_NAME"
  fi
  INSTALL_DIR="$cargo_bin"
}

# ---------------------------------------------------------------------------
# PATH setup
# ---------------------------------------------------------------------------
add_to_path_hint() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*)
      return 0
      ;;
  esac

  shell_name="$(basename "${SHELL:-sh}")"
  case "$shell_name" in
    zsh) profile="$HOME/.zshrc" ;;
    bash)
      if [ -f "$HOME/.bashrc" ]; then
        profile="$HOME/.bashrc"
      else
        profile="$HOME/.bash_profile"
      fi
      ;;
    fish) profile="$HOME/.config/fish/config.fish" ;;
    *) profile="$HOME/.profile" ;;
  esac

  export_line="export PATH=\"$INSTALL_DIR:\$PATH\""
  if [ "$shell_name" = "fish" ]; then
    export_line="fish_add_path $INSTALL_DIR"
  fi

  printf '\n'
  warn "$INSTALL_DIR is not on your PATH."
  printf '%s  Add it by running:%s\n\n' "$DIM" "$RESET"
  printf "    echo %s'%s'%s >> %s\n" "$BOLD" "$export_line" "$RESET" "$profile"
  printf '    %s\n\n' "$export_line"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
  need_cmd uname
  need_cmd tar

  target="$(detect_target)"

  if [ "${SKILLSUPPORT_FROM_SOURCE:-${EPISTEM_FROM_SOURCE:-0}}" = "1" ]; then
    install_from_source
  elif resolve_version && install_from_release "$target"; then
    :
  else
    warn "no prebuilt binary found for ${target}; falling back to source build"
    install_from_source
  fi

  installed="$INSTALL_DIR/${BIN_NAME}"
  if [ ! -x "$installed" ]; then
    error "installation failed: ${BIN_NAME} not found in ${INSTALL_DIR}"
  fi

  version="$("$installed" --version 2>/dev/null || echo "$BIN_NAME")"
  info "Installed ${BOLD}${version}${RESET} to ${INSTALL_DIR}"

  add_to_path_hint

  printf 'Get started:\n'
  printf '    %s%s learn browser-attach%s\n' "$BOLD" "$BIN_NAME" "$RESET"
}

main "$@"
