#!/bin/bash
# Generate test certificates for Pebble ACME server
#
# This script creates:
#   - pebble-ca.pem / pebble-ca-key.pem: Root CA certificate and key
#   - pebble.pem / pebble-key.pem: Pebble server TLS certificate and key
#
# The CA certificate is used by:
#   - Pebble to sign issued certificates
#   - The XDS server to trust Pebble's HTTPS endpoint

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CERT_DIR="${SCRIPT_DIR}/../certificates"

# Create certificates directory
mkdir -p "${CERT_DIR}"

echo "Generating test certificates in ${CERT_DIR}..."

# Generate Root CA
echo "Generating Root CA..."
openssl genrsa -out "${CERT_DIR}/pebble-ca-key.pem" 4096

openssl req -x509 -new -nodes \
    -key "${CERT_DIR}/pebble-ca-key.pem" \
    -sha256 \
    -days 3650 \
    -out "${CERT_DIR}/pebble-ca.pem" \
    -subj "/C=US/ST=Test/L=Test/O=Pebble Test CA/CN=Pebble Root CA"

# Generate Pebble server certificate
echo "Generating Pebble server certificate..."
openssl genrsa -out "${CERT_DIR}/pebble-key.pem" 2048

# Create CSR config with SANs
cat > "${CERT_DIR}/pebble-csr.conf" << EOF
[req]
default_bits = 2048
prompt = no
default_md = sha256
distinguished_name = dn
req_extensions = req_ext

[dn]
C = US
ST = Test
L = Test
O = Pebble Test
CN = pebble

[req_ext]
subjectAltName = @alt_names

[alt_names]
DNS.1 = pebble
DNS.2 = localhost
IP.1 = 127.0.0.1
EOF

# Create extension config for signing
cat > "${CERT_DIR}/pebble-ext.conf" << EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = pebble
DNS.2 = localhost
IP.1 = 127.0.0.1
EOF

# Generate CSR
openssl req -new \
    -key "${CERT_DIR}/pebble-key.pem" \
    -out "${CERT_DIR}/pebble.csr" \
    -config "${CERT_DIR}/pebble-csr.conf"

# Sign the certificate with the CA
openssl x509 -req \
    -in "${CERT_DIR}/pebble.csr" \
    -CA "${CERT_DIR}/pebble-ca.pem" \
    -CAkey "${CERT_DIR}/pebble-ca-key.pem" \
    -CAcreateserial \
    -out "${CERT_DIR}/pebble.pem" \
    -days 365 \
    -sha256 \
    -extfile "${CERT_DIR}/pebble-ext.conf"

# Clean up temporary files
rm -f "${CERT_DIR}/pebble.csr" "${CERT_DIR}/pebble-csr.conf" "${CERT_DIR}/pebble-ext.conf" "${CERT_DIR}/pebble-ca.srl"

# Set permissions
chmod 644 "${CERT_DIR}"/*.pem
chmod 600 "${CERT_DIR}"/*-key.pem

echo ""
echo "Certificate generation complete!"
echo ""
echo "Generated files:"
ls -la "${CERT_DIR}"
echo ""
echo "CA Certificate:"
openssl x509 -in "${CERT_DIR}/pebble-ca.pem" -noout -subject -dates
echo ""
echo "Server Certificate:"
openssl x509 -in "${CERT_DIR}/pebble.pem" -noout -subject -dates
