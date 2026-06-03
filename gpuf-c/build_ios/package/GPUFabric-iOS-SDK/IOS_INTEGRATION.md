# GPUFabric iOS SDK Integration

This package contains `gpuf_c_sdk.xcframework`, an iOS static XCFramework for GPUFabric C APIs.

## Contents

```text
gpuf_c_sdk.xcframework/
  ios-arm64/
    libgpuf_c_device.a
    Headers/
      gpuf_c.h
      gpuf_c_minimal.h
  ios-arm64-simulator/
    libgpuf_c_simulator_merged.a
    Headers/
      gpuf_c.h
      gpuf_c_minimal.h
```

Supported platforms:

- iOS device: `arm64`
- iOS simulator: `arm64`

## Add To Xcode

1. Open the iOS app project in Xcode.
2. Drag `gpuf_c_sdk.xcframework` into the project navigator.
3. In the target settings, open `General` -> `Frameworks, Libraries, and Embedded Content`.
4. Make sure `gpuf_c_sdk.xcframework` is listed.
5. Set it to `Do Not Embed`, because this SDK is a static library packaged as an XCFramework.

## Link System Libraries

In target settings, add these under `Build Phases` -> `Link Binary With Libraries`:

```text
Metal.framework
Accelerate.framework
Foundation.framework
libc++.tbd
```

If Xcode reports missing Objective-C runtime, dispatch, or pthread symbols, also add:

```text
libobjc.tbd
libSystem.tbd
```

These are normally linked automatically by iOS apps.

## Swift Bridging Header

Create a bridging header, for example `GPUFabric-Bridging-Header.h`:

```objc
#ifndef GPUFabric_Bridging_Header_h
#define GPUFabric_Bridging_Header_h

#include <gpuf_c.h>

#endif
```

Then set:

```text
Build Settings -> Swift Compiler - General -> Objective-C Bridging Header
```

Example value:

```text
$(PROJECT_DIR)/GPUFabric-Bridging-Header.h
```

## Basic Swift Usage

```swift
import Foundation

final class GPUFabricClient {
    func version() -> String {
        guard let ptr = gpuf_version() else {
            return "unknown"
        }
        return String(cString: ptr)
    }

    func startWorker(
        server: String,
        controlPort: Int32,
        proxyPort: Int32,
        workerType: String = "TCP",
        clientIdHex: String
    ) -> Int32 {
        server.withCString { serverPtr in
            workerType.withCString { workerTypePtr in
                clientIdHex.withCString { clientIdPtr in
                    start_remote_worker(
                        serverPtr,
                        controlPort,
                        proxyPort,
                        workerTypePtr,
                        clientIdPtr
                    )
                }
            }
        }
    }

    func status() -> String {
        var buffer = [CChar](repeating: 0, count: 4096)
        let rc = get_remote_worker_status(&buffer, buffer.count)
        guard rc == 0 else {
            return "error"
        }
        return String(cString: buffer)
    }

    func stopWorker() {
        _ = stop_remote_worker()
    }
}
```

## Model Loading

For local model inference, call the C APIs in this order:

```swift
let model = gpuf_load_model(modelPath)
let context = gpuf_create_context(model)
```

Keep the model path as a valid local `.gguf` file path. Model loading can consume significant memory, so test on a real device before shipping.

## Common Issues

### `Undefined symbol: std::__1...`

Add `libc++.tbd`.

### `Undefined symbol: _MTLCreateSystemDefaultDevice`

Add `Metal.framework`.

### `Undefined symbol: _cblas_sgemm...`

Add `Accelerate.framework`.

### Header Not Found

Use `#include <gpuf_c.h>` in the bridging header. If Xcode still cannot find it, confirm `gpuf_c_sdk.xcframework` is added to the app target, not only copied into the project folder.

## Build Notes

This SDK was generated from:

```text
gpuf-c/generate_llama_ios.sh
gpuf-c/generate_ios_sdk.sh
```

The final SDK path in the build machine is:

```text
/Users/jack/codedir/GPUFabric/gpuf-c/build_ios/dist/gpuf_c_sdk.xcframework
```
