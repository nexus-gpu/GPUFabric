#!/bin/bash

# generate CA key and certificate
openssl req -x509 -newkey rsa:4096 -keyout ca-key.pem -out ca-cert.pem -days 365 -nodes -subj "/CN=MyCA"

# create config file
cat > openssl.cnf <<EOL
[req]
req_extensions = v3_req
distinguished_name = req_distinguished_name

[req_distinguished_name]

[v3_req]
basicConstraints = CA:FALSE
keyUsage = nonRepudiation, digitalSignature, keyEncipherment
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
DNS.2 = ${GPUF_CERT_DNS:-agent.example.com}
IP.1 = 127.0.0.1
IP.3 = ${GPUF_CERT_IP:-127.0.0.1}
EOL

# generate server key and certificate signing request
openssl req -newkey rsa:4096 -keyout key.pem -out server.csr -nodes -subj "/CN=localhost" -config openssl.cnf

# sign server certificate with CA
openssl x509 -req -in server.csr -CA ca-cert.pem -CAkey ca-key.pem -out cert.pem -days 365 -CAcreateserial -extfile openssl.cnf -extensions v3_req

# clean up temporary files
rm server.csr openssl.cnf