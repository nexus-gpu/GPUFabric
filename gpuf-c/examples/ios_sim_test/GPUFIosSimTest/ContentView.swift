import SwiftUI
import os

private let remoteWorkerLogger = Logger(subsystem: "com.gpuf.iossimtest", category: "remote_worker")
private let environment = ProcessInfo.processInfo.environment

private final class RemoteWorkerStatusBox: @unchecked Sendable {
    var update: ((String) -> Void)?
}

private let statusBox = RemoteWorkerStatusBox()

private func remoteWorkerCallback(_ message: UnsafePointer<CChar>?, _ userData: UnsafeMutableRawPointer?) {
    guard let message else { return }
    let text = String(cString: message)
    remoteWorkerLogger.info("callback: \(text, privacy: .public)")

    DispatchQueue.main.async {
        statusBox.update?(text)
    }
}

struct ContentView: View {
    @State private var status: String = "Starting..."

    var body: some View {
        VStack(spacing: 12) {
            Text("GPUF iOS Simulator Test")
                .font(.headline)
            Text(status)
                .font(.subheadline)
                .multilineTextAlignment(.center)
                .padding()
        }
        .padding()
        .task {
            statusBox.update = { msg in
                status = msg
            }
            status = startRemoteWorkerOnce()
        }
    }
}

private func startRemoteWorkerOnce() -> String {
    let modelFileName = "Llama-3.2-1B-Instruct-Q8_0.gguf"

    let documentsModelPath: String? = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)
        .first?
        .appendingPathComponent(modelFileName)
        .path

    if let documentsModelPath, FileManager.default.fileExists(atPath: documentsModelPath) {
        remoteWorkerLogger.info("Using model from Documents: \(documentsModelPath, privacy: .public)")
        return startRemoteWorkerWithModelPath(documentsModelPath)
    }

    guard let modelPath = Bundle.main.path(forResource: "Llama-3.2-1B-Instruct-Q8_0", ofType: "gguf") else {
        remoteWorkerLogger.error("Model not found in bundle or Documents")
        let docsHint = documentsModelPath ?? "<Documents>"
        return "❌ Model not found. Add \(modelFileName) to Copy Bundle Resources, or push it to: \n\(docsHint)"
    }

    remoteWorkerLogger.info("Using model from bundle: \(modelPath, privacy: .public)")
    return startRemoteWorkerWithModelPath(modelPath)
}

private func startRemoteWorkerWithModelPath(_ modelPath: String) -> String {
    remoteWorkerLogger.info("Model path: \(modelPath, privacy: .public)")

    let serverAddr = environment["GPUF_IOS_TEST_SERVER_ADDR"] ?? "127.0.0.1"
    let controlPort = Int32(environment["GPUF_IOS_TEST_CONTROL_PORT"] ?? "17100") ?? 17100
    let proxyPort = Int32(environment["GPUF_IOS_TEST_PROXY_PORT"] ?? "17101") ?? 17101
    let workerType = "TCP"
    let clientId = environment["GPUF_IOS_TEST_CLIENT_ID"] ?? "00112233445566778899aabbccddeeff"
    let useTLS = environment["GPUF_IOS_TEST_TLS"] == "1"
    let caCertPath = environment["GPUF_IOS_TEST_CA_CERT_PATH"]
    let controlTLSServerName = environment["GPUF_IOS_TEST_TLS_SERVER_NAME"] ?? serverAddr
    let certSHA256Pin = environment["GPUF_IOS_TEST_CERT_SHA256_PIN"]

    remoteWorkerLogger.info("Starting remote worker with clientId: \(clientId, privacy: .public), tls: \(useTLS)")

    let modelRc = modelPath.withCString { cstr in
        set_remote_worker_model(cstr)
    }
    if modelRc != 0 {
        remoteWorkerLogger.error("set_remote_worker_model failed: \(modelRc)")
        return "❌ set_remote_worker_model failed: \(modelRc)"
    }

    let startRc: Int32
    if useTLS {
        startRc = serverAddr.withCString { s in
            workerType.withCString { w in
                clientId.withCString { c in
                    controlTLSServerName.withCString { tlsName in
                        withOptionalCString(caCertPath) { ca in
                            withOptionalCString(certSHA256Pin) { pin in
                                start_remote_worker_with_tls(s, controlPort, proxyPort, w, c, ca, tlsName, pin)
                            }
                        }
                    }
                }
            }
        }
    } else {
        startRc = serverAddr.withCString { s in
            workerType.withCString { w in
                clientId.withCString { c in
                    start_remote_worker(s, controlPort, proxyPort, w, c)
                }
            }
        }
    }
    if startRc != 0 {
        remoteWorkerLogger.error("start_remote_worker failed: \(startRc)")
        return "❌ start_remote_worker failed: \(startRc)"
    }

    let cb: (@convention(c) (UnsafePointer<CChar>?, UnsafeMutableRawPointer?) -> Void) = remoteWorkerCallback
    let registerRc = gpuf_register_remote_worker_callback(cb, nil)
    if registerRc != 0 {
        remoteWorkerLogger.error("gpuf_register_remote_worker_callback failed: \(registerRc)")
        return "❌ gpuf_register_remote_worker_callback failed: \(registerRc)"
    }

    let tasksRc = start_remote_worker_tasks()
    if tasksRc != 0 {
        remoteWorkerLogger.error("start_remote_worker_tasks failed: \(tasksRc)")
        return "❌ start_remote_worker_tasks failed: \(tasksRc)"
    }

    var buffer = [CChar](repeating: 0, count: 512)
    let statusRc = buffer.withUnsafeMutableBufferPointer { buf in
        get_remote_worker_status(buf.baseAddress, buf.count)
    }
    if statusRc == 0 {
        let statusText = String(cString: buffer)
        remoteWorkerLogger.info("get_remote_worker_status: \(statusText, privacy: .public)")
        return "✅ Remote worker started\n\(statusText)"
    }

    return "✅ Remote worker started"
}

private func withOptionalCString<R>(_ value: String?, _ body: (UnsafePointer<CChar>?) -> R) -> R {
    guard let value, !value.isEmpty else {
        return body(nil)
    }
    return value.withCString { cstr in
        body(cstr)
    }
}
