
# GPUFabric Mobile SDK

## Documents

- [Build Guide](./BUILD_GUIDE.md)
- [Integration Guide (EN)](./INTEGRATION_GUIDE_EN.md)

## Security Status

The native SDK interface remains compatible after the 2026-06-04 remediation. Release packages must be verified with `SHA256SUMS`; explicit server addresses are required for remote worker integrations. Existing C/JNI remote worker start APIs remain unchanged for plaintext compatibility. Production mobile wrappers can opt into TLS with the additive `start_remote_worker_with_tls` / `RemoteWorker.startRemoteWorkerWithTls` APIs after preflighting CA/SNI/SHA256 pin inputs with `gpuf_validate_mobile_tls_policy` / `RemoteWorker.validateMobileTlsPolicy`. The iOS SDK build now uses the additive `ios-sdk` feature by default and compiles with `--no-default-features` so it links prebuilt llama.cpp archives from `target/llama-ios/`; generated `build_ios/dist/`, `build_ios/package/`, `build_llama_ios/`, and Xcode `DerivedData/` outputs are ignored and must be distributed through release artifacts, not committed. Android arm64 target compile, packaged SDK local inference, plaintext Remote Worker, additive TLS Remote Worker, and Linux nightly ASAN/TSAN mobile unit tests pass locally; Android raw logs and sanitizer evidence are attached under `security-release-evidence/mobile-sdk/evidence/`. iOS runtime logs, Android/iOS runtime sanitizer or release-owner substitute approval, production Android Keystore/iOS Keychain wrapper sign-off, platform permission/logging audits, and production signing remain release gates. Use `scripts/mobile_sdk_release_gate.sh` in release jobs; set `GPUF_REQUIRE_MOBILE_EVIDENCE=1` for formal mobile SDK distribution. Android SDK archive/SHA256 evidence is available locally for `target/gpufabric-android-sdk-v9.0.0.tar.gz`.

