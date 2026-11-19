#if os(iOS)
@preconcurrency import RelayFFI
import CoreImage.CIFilterBuiltins
import ForgeAppKit
import SwiftUI
import UIKit
import Vision
import VisionKit

private let scanLog = ForgeLog("Scanner")

/// A word code as a QR code, for the receiver to scan instead of typing.
enum CodeQR {
    static func image(_ text: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 10, y: 10)),
              let image = CIContext().createCGImage(output, from: output.extent)
        else { return nil }
        return UIImage(cgImage: image)
    }
}

/// VisionKit's live scanner, reporting the first word code it sees (one
/// Rust can read).
struct CodeScanner: UIViewControllerRepresentable {
    let found: (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(found: found) }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        do {
            try scanner.startScanning()
        } catch {
            // Checked as available just before; typing the code still works.
            scanLog.warn("The camera scanner didn't start: \(error)")
        }
        return scanner
    }

    func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {
        context.coordinator.found = found
    }

    static func dismantleUIViewController(_ scanner: DataScannerViewController, coordinator: Coordinator) {
        scanner.stopScanning()
    }

    @MainActor
    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        var found: (String) -> Void
        private var reported: String?

        init(found: @escaping (String) -> Void) {
            self.found = found
        }

        func dataScanner(_ scanner: DataScannerViewController, didAdd added: [RecognizedItem], allItems: [RecognizedItem]) {
            for item in added {
                guard case .barcode(let code) = item,
                      let text = code.payloadStringValue?.trimmingCharacters(in: .whitespacesAndNewlines),
                      (try? wordCodeNumber(code: text)) != nil,
                      text != reported
                else { continue }
                reported = text
                found(text)
                return
            }
        }
    }
}
#endif
