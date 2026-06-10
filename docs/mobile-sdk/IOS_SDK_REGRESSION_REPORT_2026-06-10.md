# iOS SDK Regression Report - 2026-06-10

## Scope

- Branch: `optimize/gpuf-c`
- Commit: `e1fc8b9eb58867f4e3749b827eca5b399ef45c7d`
- Objective: package release-gate evidence for the iOS SDK regression work already performed, including local SDK build, C ABI availability, simulator integration flow, TLS control-stream coverage, and mobile security review notes.
- Evidence directory for `scripts/mobile_sdk_release_gate.sh`: `security-release-evidence/mobile-sdk/evidence`

## Summary

- iOS SDK build: PASS on local macOS host.
- Generated artifact: `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework`.
- Included slices: `ios-arm64` and `ios-arm64-simulator`, both using the unified archive name `libgpuf_c_sdk.a`.
- Intel simulator slice: NOT INCLUDED. `x86_64-apple-ios` was skipped because `target/llama-ios/x86_64-apple-ios` is not present locally.
- C ABI symbols: present for local inference, Remote Worker, TLS Remote Worker, and C callback registration.
- Simulator/device runtime: previous plain/TLS Remote Worker flow was exercised with a real GGUF model, but raw simulator runtime logs were not persisted into a file. This report records the flow and marks raw runtime log attachment as a release evidence follow-up.
- TLS policy unit tests: PASS on 2026-06-10 Linux re-check after syncing the certificate fixture; `cargo test -p gpuf-c mobile_tls_policy --lib` passed 7/7 tests.

## Local Build Evidence

Toolchain:

- macOS: `26.3.1` (`25D2128`)
- Xcode: `26.3` (`17C529`)
- iPhoneOS SDK: `iPhoneOS26.2.sdk`
- iPhoneSimulator SDK: `iPhoneSimulator26.2.sdk`
- Rust: `rustc 1.93.1 (01f6ddf75 2026-02-11)`
- Cargo: `cargo 1.93.1 (083ac5135 2025-12-15)`

Command:

```bash
cd <repo>/gpuf-c
GPUF_SKIP_CBINDGEN=1 bash generate_ios_sdk.sh
```

Result:

- Device target `aarch64-apple-ios`: build succeeded.
- Simulator target `aarch64-apple-ios-sim`: build succeeded.
- `x86_64-apple-ios`: skipped, missing llama.cpp iOS libs at `target/llama-ios/x86_64-apple-ios`.
- XCFramework creation succeeded.

Generated files:

- `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework/Info.plist`
- `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework/ios-arm64/libgpuf_c_sdk.a`
- `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework/ios-arm64/Headers/gpuf_c.h`
- `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework/ios-arm64/Headers/gpuf_c_minimal.h`
- `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework/ios-arm64-simulator/libgpuf_c_sdk.a`
- `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework/ios-arm64-simulator/Headers/gpuf_c.h`
- `gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework/ios-arm64-simulator/Headers/gpuf_c_minimal.h`

Artifact SHA256:

```text
0ae77c1a27832b1ba671f87e595c9a6c5357a64cc96ba2573be43ad2284e7715  gpuf_c_sdk.xcframework/ios-arm64/libgpuf_c_sdk.a
a249be9d2835b28acbe67d1ac8be4518a27a1d5c627addb2f5882a6e581358f8  gpuf_c_sdk.xcframework/ios-arm64-simulator/libgpuf_c_sdk.a
36443743991985fc4a38bade73037e062c25d3a365e09d8548f7407b90f9aded  gpuf_c_sdk.xcframework/*/Headers/gpuf_c.h
7f5bdf408fc16d1d283c9e55f7e1e8b6e72c2a0c7c0601cbcb3fc0890c5aafbd  gpuf_c_sdk.xcframework/*/Headers/gpuf_c_minimal.h
958424e62774868335dc4dbdc27701dc24260bfa7dccf2d70a4d2f4f4f970d27  gpuf_c_sdk.xcframework/Info.plist
```

Slice check:

```text
ios-arm64/libgpuf_c_sdk.a: architecture arm64
ios-arm64-simulator/libgpuf_c_sdk.a: architecture arm64
```

## ABI Evidence

The iOS header `gpuf_c.h` exposes pure C APIs without Android/JNI types:

- `gpuf_init`
- `gpuf_cleanup`
- `gpuf_version`
- `gpuf_system_info`
- `gpuf_load_model`
- `gpuf_create_context`
- `gpuf_generate_final_solution_text`
- `llama_model_free`
- `llama_free`
- `set_remote_worker_model`
- `start_remote_worker`
- `start_remote_worker_with_tls`
- `gpuf_validate_mobile_tls_policy`
- `start_remote_worker_tasks`
- `gpuf_register_remote_worker_callback`
- `get_remote_worker_status`
- `stop_remote_worker`

Symbol scan on `ios-arm64-simulator/libgpuf_c_sdk.a` found the expected exported symbols. Apple `nm` emitted LLVM attribute warnings for Rust `compiler_builtins` object files, but still listed the expected GPUFabric and llama symbols; this warning is a tool-version compatibility issue, not a missing-symbol result.

## iOS Runtime Flow Evidence

The simulator sample app at `gpuf-c/examples/ios_sim_test` exercises:

1. Model path discovery from Simulator Documents or bundle.
2. `set_remote_worker_model(modelPath)`.
3. Plain flow: `start_remote_worker(server, controlPort, proxyPort, "TCP", clientId)`.
4. TLS flow: `start_remote_worker_with_tls(server, controlPort, proxyPort, "TCP", clientId, caCertPath, serverName, certSha256Pin)`.
5. Callback registration with `gpuf_register_remote_worker_callback`.
6. Worker task start with `start_remote_worker_tasks`.
7. Status query with `get_remote_worker_status`.

Previous local regression flow used a real GGUF model (`Llama-3.2-1B-Instruct-Q8_0.gguf`) and covered plain and TLS Remote Worker startup. The raw runtime console logs were not persisted under the evidence directory, so final release evidence should add a saved `run_ios_sim_test.sh` / `run_ios_sim_tls_test.sh` log or Xcode Console export.

Current sandbox re-query of `xcrun simctl list devices` failed with `CoreSimulatorService connection became invalid`, so this report does not claim a new simulator runtime pass on 2026-06-10.

## TLS Evidence

Additive TLS APIs are present:

- C: `gpuf_validate_mobile_tls_policy`
- C: `start_remote_worker_with_tls`
- Android JNI wrapper remains additive and source-compatible.
- Plain `start_remote_worker` remains available for compatibility.

TLS policy unit test command:

```bash
cargo test -p gpuf-c mobile_tls_policy --lib
```

Result:

- Re-check host: Linux workspace checkout (`<repo>`).
- Re-check commit: `52e6697ee14fd5714a904280947f7861b84e7ee3`.
- 7 passed.
- 0 failed.
- Passing coverage includes valid CA bundle + SHA256 pin and stable C FFI error-code mapping.

Passing tests:

- `accepts_ca_bundle_and_sha256_pin`
- `accepts_colon_separated_pin_without_ca`
- `ffi_returns_stable_error_codes`
- `rejects_bad_pin`
- `rejects_invalid_server_name`
- `rejects_missing_ca_file`
- `rejects_missing_trust_material`

Conclusion: TLS API and policy unit coverage is green for CA bundle parsing, SHA256 pin normalization, invalid policy rejection, and stable C return codes. Human release review should still attach raw iOS TLS runtime logs for the simulator/device flow described above.

## Security Review Notes

- iOS sample `Info.plist` declares no privacy-sensitive permissions.
- iOS SDK code does not store long-lived token material in Keychain, UserDefaults, or plaintext files.
- Production wrappers should store long-lived credentials in Keychain and pass only short-lived or user-scoped runtime values to the SDK.
- The simulator example currently logs callback text, model path, and a demo client identifier with public privacy. This is acceptable for local diagnostics only; production app logs must redact client IDs, tokens, prompts, model paths, and server-specific endpoints.

## Release Gate Status

The evidence files required by `scripts/mobile_sdk_release_gate.sh` are populated in:

```text
security-release-evidence/mobile-sdk/evidence/
```

This evidence package is suitable for the script-level presence check. Human release approval should still review the conditional items:

- Attach raw iOS simulator/device runtime logs for plain and TLS Remote Worker runs.
- Add Android instrumentation logs if this is a full Android+iOS mobile SDK release.
- Add ASAN/TSAN or document an approved sanitizer substitute for mobile FFI callback lifecycle tests.
