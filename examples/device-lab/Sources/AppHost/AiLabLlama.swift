import Foundation
import llama

/// Build spike for running GGUF models (Ternary-Bonsai Q2_0) on the phone
/// with llama.cpp's prebuilt XCFramework. For now it only proves that the
/// framework builds with xtool, links, and finds the Metal GPU at run time;
/// no model is loaded yet.
enum LlamaProbe {
    /// One line for AI Lab: the GPU devices llama.cpp found, or why not.
    static func describe() -> String {
        llama_backend_init()
        var devices: [String] = []
        for index in 0..<ggml_backend_dev_count() {
            guard let device = ggml_backend_dev_get(index) else { continue }
            devices.append(String(cString: ggml_backend_dev_name(device)))
        }
        return devices.isEmpty ? "No backends" : devices.joined(separator: ", ")
    }

    /// llama.cpp's own build and CPU feature line, for the log.
    static func systemInfo() -> String {
        String(cString: llama_print_system_info())
    }
}
