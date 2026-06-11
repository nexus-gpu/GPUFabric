# GPUFabric iOS SDK Test Environment Parameters

Updated: 2026-06-11

This file contains the iOS SDK TLS test-environment parameter template. Fill
concrete endpoint and certificate values from the secure deployment handoff, not
from committed docs.

## Runtime Parameters

```text
server_addr = <test-gpuf-s-host-or-ip>
control_port = 17100
proxy_port = 17101
worker_type = TCP
control_tls_server_name = <test-control-tls-server-name>
ca_cert_path = <absolute path to certs/control-ca.pem after copying it into the app bundle or sandbox>
cert_sha256_pin = ""
client_id = <32-char hex client id assigned by backend/test environment>
```

Use the CA bundle included in this package:

```text
certs/control-ca.pem
```

`cert_sha256_pin` can stay empty when `ca_cert_path` points to this PEM file.

## Test CA

```text
subject = <test-ca-subject>
issuer = <test-ca-issuer>
notBefore = <test-ca-not-before>
notAfter = <test-ca-not-after>
sha256 fingerprint =
<test-ca-sha256-fingerprint>
```

## Test Server Certificate

```text
subject = <test-server-subject>
issuer = <test-ca-issuer>
SAN = <test-server-san-list>
notBefore = <test-server-not-before>
notAfter = <test-server-not-after>
sha256 fingerprint =
<test-server-cert-sha256-fingerprint>
```

## Swift Configuration Example

```swift
let serverAddr = "<test-gpuf-s-host-or-ip>"
let controlPort: Int32 = 17100
let proxyPort: Int32 = 17101
let workerType = "TCP"
let controlTLSServerName = "<test-control-tls-server-name>"
let certSHA256Pin = ""

let caCertPath = Bundle.main.url(
    forResource: "control-ca",
    withExtension: "pem"
)!.path
```

## Production Mapping

```text
server_addr: production gpuf-s DNS name or IP. DNS is preferred.
control_port: production gpuf-s control TLS port.
proxy_port: production gpuf-s proxy/data port.
control_tls_server_name: production DNS name covered by the server certificate SAN.
ca_cert_path: absolute path to the production CA bundle in the app bundle or sandbox.
cert_sha256_pin: optional production server certificate SHA256 pin.
```

Do not use the test CA in production. Do not log full client ids, tokens,
prompts, production pins, or private keys.
