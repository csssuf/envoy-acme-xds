#!/bin/bash
# Cleanup script for envoy-acme-xds test environment
#
# Usage:
#   ./test/cleanup.sh           # Standard cleanup (preserve Pebble CA)
#   ./test/cleanup.sh --full    # Full cleanup (remove Pebble CA too)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="${SCRIPT_DIR}/.."
CERT_DIR="${PROJECT_DIR}/certificates"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Parse arguments
FULL_CLEANUP=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --full|-f) FULL_CLEANUP=true; shift ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Clean up test environment and reset to initial state."
            echo ""
            echo "Options:"
            echo "  --full, -f     Full cleanup - also remove Pebble CA certificates"
            echo "  --help, -h     Show this help message"
            echo ""
            echo "Standard cleanup:"
            echo "  - Stops all containers"
            echo "  - Removes xds-data volume (ACME account + issued certificates)"
            echo "  - Keeps Pebble CA certificates"
            echo ""
            echo "Full cleanup (--full):"
            echo "  - Everything from standard cleanup"
            echo "  - Also removes Pebble CA certificates"
            echo "  - Run ./test/generate-certs.sh before next test"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

cd "${PROJECT_DIR}"

log_info "Starting cleanup..."

# Step 1: Stop containers
log_info "Stopping containers..."
if podman compose ps --quiet | grep -q .; then
    podman compose down
    log_info "Containers stopped"
else
    log_info "No containers running"
fi

# Step 2: Remove xds-data volume
VOLUME_NAME="envoy-acme-xds_xds-data"
if podman volume exists "${VOLUME_NAME}" 2>/dev/null; then
    log_warn "Removing volume: ${VOLUME_NAME}"
    log_warn "This will delete:"
    log_warn "  - ACME account credentials (account.json)"
    log_warn "  - All issued certificates (certs/)"

    read -p "Continue? [y/N] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        podman volume rm "${VOLUME_NAME}"
        log_info "Volume removed"
    else
        log_info "Skipping volume removal"
    fi
else
    log_info "Volume ${VOLUME_NAME} does not exist"
fi

# Step 3: Optionally remove Pebble CA certificates
if [[ "${FULL_CLEANUP}" == "true" ]]; then
    if [[ -d "${CERT_DIR}" ]] && [[ -n "$(ls -A "${CERT_DIR}" 2>/dev/null)" ]]; then
        log_warn "Full cleanup - removing Pebble CA certificates"
        log_warn "Directory: ${CERT_DIR}"
        log_warn "Run ./test/generate-certs.sh before next test"

        read -p "Continue? [y/N] " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            rm -rf "${CERT_DIR}"/*
            log_info "Pebble CA certificates removed"
        else
            log_info "Skipping Pebble CA removal"
        fi
    else
        log_info "No Pebble CA certificates to remove"
    fi
else
    log_info "Keeping Pebble CA certificates (use --full to remove)"
fi

echo ""
log_info "Cleanup complete!"
echo ""
log_info "Next steps:"
if [[ "${FULL_CLEANUP}" == "true" ]]; then
    echo "  1. Generate certificates: ./test/generate-certs.sh"
    echo "  2. Start services:        ./test/run-test.sh"
else
    echo "  1. Start services: ./test/run-test.sh"
fi
