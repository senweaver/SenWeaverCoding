#!/usr/bin/env bash
# SenWeaverCoding CLI installer — Linux & macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/senweaver/SenWeaverCoding/master/install.sh | bash
set -euo pipefail

REPO="senweaver/SenWeaverCoding"
BINARY_NAME="sen"
INSTALL_DIR="${SEN_INSTALL_DIR:-$HOME/.local/bin}"

info()  { printf "\033[1;34m==>\033[0m %s\n" "$*"; }
ok()    { printf "\033[1;32m==>\033[0m %s\n" "$*"; }
err()   { printf "\033[1;31merror:\033[0m %s\n" "$*" >&2; exit 1; }

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)   os="unknown-linux-gnu" ;;
        Darwin)  os="apple-darwin" ;;
        *)       err "Unsupported OS: $os" ;;
    esac

    case "$arch" in
        x86_64|amd64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *)             err "Unsupported architecture: $arch" ;;
    esac

    echo "${arch}-${os}"
}

fetch() {
    if command -v curl &>/dev/null; then
        curl -fsSL "$1"
    elif command -v wget &>/dev/null; then
        wget -qO- "$1"
    else
        err "Neither curl nor wget found. Please install one of them."
    fi
}

download() {
    if command -v curl &>/dev/null; then
        curl -fsSL -o "$2" "$1"
    else
        wget -qO "$2" "$1"
    fi
}

main() {
    info "Detecting platform..."
    local triple
    triple="$(detect_platform)"
    info "Platform: $triple"

    # Determine version
    local version="${SEN_VERSION:-latest}"
    local api_url

    if [ "$version" = "latest" ]; then
        api_url="https://api.github.com/repos/${REPO}/releases/latest"
    else
        api_url="https://api.github.com/repos/${REPO}/releases/tags/v${version}"
    fi

    info "Fetching release info..."
    local release_json
    release_json="$(fetch "$api_url")" || err "Failed to fetch release info from GitHub."

    # Find the asset URL matching our platform
    local asset_url
    asset_url="$(echo "$release_json" | grep -o "\"browser_download_url\"[[:space:]]*:[[:space:]]*\"[^\"]*${triple}[^\"]*\"" | head -1 | sed 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\(.*\)"/\1/')"

    if [ -z "$asset_url" ]; then
        err "No release asset found for ${triple}. Check https://github.com/${REPO}/releases"
    fi

    info "Downloading ${BINARY_NAME} from ${asset_url}..."
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    local archive="${tmp_dir}/sen.tar.gz"
    download "$asset_url" "$archive"

    info "Extracting..."
    tar -xzf "$archive" -C "$tmp_dir"

    # Find the binary in extracted files
    local binary
    binary="$(find "$tmp_dir" -name "$BINARY_NAME" -type f | head -1)"
    if [ -z "$binary" ]; then
        binary="$(find "$tmp_dir" -name "${BINARY_NAME}.exe" -type f | head -1)"
    fi
    if [ -z "$binary" ]; then
        err "Binary '${BINARY_NAME}' not found in archive."
    fi

    info "Installing to ${INSTALL_DIR}/${BINARY_NAME}..."
    mkdir -p "$INSTALL_DIR"
    cp "$binary" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

    # Check if INSTALL_DIR is in PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        printf "\n"
        ok "Installed! Add %s to your PATH:" "$INSTALL_DIR"
        echo ""
        echo "  # bash/zsh:"
        echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
        echo ""
        echo "  # fish:"
        echo "  fish_add_path ${INSTALL_DIR}"
        echo ""
    else
        ok "Installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
    fi

    local installed_version
    installed_version="$("${INSTALL_DIR}/${BINARY_NAME}" --version 2>/dev/null || echo "unknown")"
    ok "Version: ${installed_version}"
    echo ""
    echo "Get started:"
    echo "  ${BINARY_NAME} onboard        # first-time setup"
    echo "  ${BINARY_NAME} agent           # start interactive session"
    echo "  ${BINARY_NAME} agent -m 'hi'   # single message"
}

main "$@"
