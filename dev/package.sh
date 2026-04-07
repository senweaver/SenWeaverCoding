#!/usr/bin/env bash
# Local packaging script — build all package formats for the current platform.
#
# Usage:
#   ./dev/package.sh           # build all available formats
#   ./dev/package.sh deb       # build .deb only
#   ./dev/package.sh rpm       # build .rpm only
#   ./dev/package.sh msi       # build .msi only (Windows)
#   ./dev/package.sh tar       # build .tar.gz only
#   ./dev/package.sh all       # build everything
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

info()  { printf "\033[1;34m==>\033[0m %s\n" "$*"; }
ok()    { printf "\033[1;32m==>\033[0m %s\n" "$*"; }
err()   { printf "\033[1;31merror:\033[0m %s\n" "$*" >&2; exit 1; }

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
TARGET=$(rustc -vV | awk '/^host:/ { print $2 }')
DIST_DIR="$PROJECT_DIR/dist"

mkdir -p "$DIST_DIR"

build_binary() {
    info "Building release binary for ${TARGET}..."
    cargo build --release --target "$TARGET"
    ok "Binary built: target/${TARGET}/release/sen"
}

package_tar() {
    info "Creating .tar.gz archive..."
    local archive="$DIST_DIR/sen-${VERSION}-${TARGET}.tar.gz"
    tar -czf "$archive" -C "target/${TARGET}/release" sen
    ok "Archive: $archive"
}

package_deb() {
    if ! command -v cargo-deb &>/dev/null; then
        info "Installing cargo-deb..."
        cargo install cargo-deb --locked
    fi
    info "Building .deb package..."
    cargo deb --no-build --target "$TARGET"
    local deb
    deb=$(find "target/${TARGET}/debian" -name "*.deb" | head -1)
    cp "$deb" "$DIST_DIR/"
    ok "Deb package: $DIST_DIR/$(basename "$deb")"
}

package_rpm() {
    if ! command -v cargo-generate-rpm &>/dev/null; then
        info "Installing cargo-generate-rpm..."
        cargo install cargo-generate-rpm --locked
    fi
    info "Building .rpm package..."
    cargo generate-rpm --target "$TARGET"
    local rpm
    rpm=$(find "target/${TARGET}/generate-rpm" -name "*.rpm" | head -1)
    cp "$rpm" "$DIST_DIR/"
    ok "RPM package: $DIST_DIR/$(basename "$rpm")"
}

package_msi() {
    err "MSI packaging requires Windows + WiX Toolset v6. Use: powershell -File dev/package-windows.ps1"
}

mode="${1:-all}"

case "$mode" in
    tar)
        build_binary
        package_tar
        ;;
    deb)
        build_binary
        package_deb
        ;;
    rpm)
        build_binary
        package_rpm
        ;;
    msi)
        package_msi
        ;;
    all)
        build_binary
        package_tar

        os="$(uname -s)"
        case "$os" in
            Linux)
                package_deb
                package_rpm
                ;;
            Darwin)
                ok "macOS: .tar.gz created. For .pkg packaging, use Xcode's productbuild."
                ;;
        esac
        ;;
    *)
        err "Unknown format: $mode. Use: tar, deb, rpm, msi, all"
        ;;
esac

echo ""
ok "Packages available in: $DIST_DIR/"
ls -lh "$DIST_DIR/"
