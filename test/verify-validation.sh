#!/bin/bash
# Verify real ACME HTTP-01 validation is working
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="${SCRIPT_DIR}/.."

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_test() { echo -e "${BLUE}[TEST]${NC} $1"; }
log_pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
log_fail() { echo -e "${RED}[FAIL]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }

cd "${PROJECT_DIR}"

log_info "Verifying real ACME HTTP-01 validation..."
echo ""

# Test 1: PEBBLE_VA_ALWAYS_VALID not set
log_test "Test 1: Verify PEBBLE_VA_ALWAYS_VALID is disabled"
if podman compose config | grep -q "PEBBLE_VA_ALWAYS_VALID"; then
    log_fail "PEBBLE_VA_ALWAYS_VALID is still set"
    exit 1
else
    log_pass "PEBBLE_VA_ALWAYS_VALID is not set"
fi
echo ""

# Test 2: DNS configuration
log_test "Test 2: Verify DNS configuration in Pebble"
HOSTS=$(podman exec envoy-acme-xds_pebble_1 cat /etc/hosts)
EXPECTED_DOMAINS=("site-a.example.com" "www.site-a.example.com" "site-b.example.com" "api.example.com")
ALL_FOUND=true

for domain in "${EXPECTED_DOMAINS[@]}"; do
    if echo "${HOSTS}" | grep -q "${domain}"; then
        log_pass "  ${domain} in /etc/hosts"
    else
        log_fail "  ${domain} NOT in /etc/hosts"
        ALL_FOUND=false
    fi
done

if [[ "${ALL_FOUND}" == "false" ]]; then
    exit 1
fi
echo ""

# Test 3: Connectivity
log_test "Test 3: Verify Pebble can reach Envoy"
if podman exec envoy-acme-xds_pebble_1 wget -q -O- --timeout=5 http://site-a.example.com:5001/ >/dev/null 2>&1; then
    log_pass "Pebble can reach Envoy on port 5001"
else
    log_warn "Cannot reach Envoy (may be normal if redirecting)"
fi
echo ""

# Test 4: Validation attempts in logs
log_test "Test 4: Check for HTTP-01 validation attempts"
LOGS=$(podman compose logs pebble 2>&1 | grep "GET /.well-known/acme-challenge/" || true)

if [[ -n "${LOGS}" ]]; then
    log_pass "Found validation attempts in logs"
    echo ""
    echo "Sample validation requests:"
    echo "${LOGS}" | head -3
else
    log_warn "No validation attempts yet (normal if certs not requested)"
fi
echo ""

# Test 5: Certificates issued
log_test "Test 5: Check if certificates were issued"
if podman exec envoy-acme-xds_xds-server_1 test -d /var/lib/envoy-acme-xds/certs/site-a 2>/dev/null; then
    CERT_COUNT=$(podman exec envoy-acme-xds_xds-server_1 find /var/lib/envoy-acme-xds/certs -name "cert.pem" 2>/dev/null | wc -l)
    log_pass "Found ${CERT_COUNT} issued certificate(s)"
else
    log_warn "No certificates issued yet"
    log_warn "Run './test/run-test.sh' to trigger issuance"
fi
echo ""

echo "========================================"
log_pass "Real HTTP-01 validation is enabled!"
echo "========================================"
echo ""
log_info "To see validation in action:"
echo "  1. Clean environment:     ./test/cleanup.sh"
echo "  2. Start services:        ./test/run-test.sh --keep"
echo "  3. Watch validation:      podman compose logs -f pebble | grep 'GET /.well-known'"
