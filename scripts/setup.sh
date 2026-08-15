#!/usr/bin/env bash
set -Eeuo pipefail

umask 022

INSTALL_PROVIDERS=1
INSTALL_RUST=1
PREFIX="${HOME}/.local"

usage() {
  echo "Usage: ./scripts/setup.sh [--skip-providers] [--skip-rust] [--prefix PATH]"
}

while (($#)); do
  case "$1" in
    --skip-providers) INSTALL_PROVIDERS=0 ;;
    --skip-rust) INSTALL_RUST=0 ;;
    --prefix)
      shift
      [[ $# -gt 0 ]] || { echo "--prefix requires a path" >&2; exit 2; }
      PREFIX="$1"
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

case "$(uname -s)" in
  Linux)
    command -v bwrap >/dev/null || {
      echo "Missing required command: bwrap (install the bubblewrap package for fail-closed provider isolation)." >&2
      exit 1
    }
    ;;
  Darwin)
    command -v sandbox-exec >/dev/null || {
      echo "Missing required command: sandbox-exec (required for fail-closed provider isolation)." >&2
      exit 1
    }
    ;;
  *) echo "This setup script supports macOS, Linux, and WSL. See README.md for Windows." >&2; exit 1 ;;
esac

for dependency in curl git; do
  command -v "$dependency" >/dev/null || { echo "Missing required command: $dependency" >&2; exit 1; }
done

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf -- "${TEMP_DIR}"' EXIT

if ! command -v cargo >/dev/null; then
  if ((INSTALL_RUST == 0)); then
    echo "Rust is required but cargo is not installed." >&2
    exit 1
  fi
  echo "Installing the Rust toolchain for the current user..."
  curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs -o "${TEMP_DIR}/rustup-init.sh"
  sh "${TEMP_DIR}/rustup-init.sh" -y --profile minimal
  # shellcheck disable=SC1091
  source "${HOME}/.cargo/env"
fi

if ((INSTALL_PROVIDERS == 1)); then
  if ! command -v codex >/dev/null; then
    echo "Installing Codex CLI with the official standalone installer..."
    curl --proto '=https' --tlsv1.2 -fsSL https://chatgpt.com/codex/install.sh -o "${TEMP_DIR}/codex-install.sh"
    sh "${TEMP_DIR}/codex-install.sh"
  fi
  if ! command -v claude >/dev/null; then
    echo "Installing Claude Code stable with the official native installer..."
    curl --proto '=https' --tlsv1.2 -fsSL https://claude.ai/install.sh -o "${TEMP_DIR}/claude-install.sh"
    bash "${TEMP_DIR}/claude-install.sh" stable
  fi
fi

mkdir -p "${PREFIX}/bin"
echo "Building agent-loop..."
cargo build --release --manifest-path "${REPOSITORY_ROOT}/Cargo.toml"
install -m 0755 "${REPOSITORY_ROOT}/target/release/agent-loop" "${PREFIX}/bin/agent-loop"

case ":${PATH}:" in
  *":${PREFIX}/bin:"*) ;;
  *)
    echo
    echo "Add this line to your shell profile, then open a new terminal:"
    echo "  export PATH=\"${PREFIX}/bin:\${PATH}\""
    export PATH="${PREFIX}/bin:${PATH}"
    ;;
esac

SHELL_NAME="$(basename -- "${SHELL:-bash}")"
case "${SHELL_NAME}" in
  bash)
    COMPLETION_DIR="${PREFIX}/share/bash-completion/completions"
    mkdir -p "${COMPLETION_DIR}"
    agent-loop completions bash > "${COMPLETION_DIR}/agent-loop"
    ;;
  zsh)
    COMPLETION_DIR="${PREFIX}/share/zsh/site-functions"
    mkdir -p "${COMPLETION_DIR}"
    agent-loop completions zsh > "${COMPLETION_DIR}/_agent-loop"
    ;;
  fish)
    COMPLETION_DIR="${PREFIX}/share/fish/vendor_completions.d"
    mkdir -p "${COMPLETION_DIR}"
    agent-loop completions fish > "${COMPLETION_DIR}/agent-loop.fish"
    ;;
esac

echo
echo "agent-loop installed successfully."
echo "Authenticate providers once if needed:"
echo "  codex login"
echo "  claude"
echo
echo "Then add a repository:"
echo "  cd /path/to/repository"
echo "  agent-loop init"
echo "  agent-loop doctor"
