#if os(iOS)
@preconcurrency import RelayFFI
import CoreImage.CIFilterBuiltins
import ForgeAppKit
import SwiftUI
import UIKit
import Vision
import VisionKit
import ForgeUI

private let scanLog = ForgeLog("Scanner")

/// Shows this device's pairing code as a QR code for the other device to
/// scan. The code works once, for five minutes; it closes itself when the
/// other device has paired.
struct ShowCodeSheet: View {
    @Bindable var model: RelayModel
    @Environment(\.dismiss) private var dismiss
    @State private var pairedBefore = 0

    var body: some View {
        NavigationStack {
            VStack(spacing: 20) {
                if let invitation = model.invitation, let image = Self.qrCode(invitation.code) {
                    Image(uiImage: image)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: 280)
                        .padding()
                        .background(.white, in: RoundedRectangle(cornerRadius: 16))
                        .accessibilityLabel("Pairing code for \(model.identity.name)")
                        .accessibilityIdentifier("code.qr")
                    Text("On the other device, open Relay, tap Pair a Device, then Scan a Code.")
                        .multilineTextAlignment(.center)
                    Text("Works once, until \(RelayFormat.date(invitation.expiresMs).formatted(date: .omitted, time: .shortened)).")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    ShareLink(item: invitation.code) {
                        Label("Share as Text", systemImage: "square.and.arrow.up")
                    }
                    .accessibilityIdentifier("code.share")
                } else {
                    ContentUnavailableView(
                        "No Code",
                        systemImage: "wifi.slash",
                        description: Text("Relay needs a Wi-Fi network to make a pairing code.")
                    )
                }
            }
            .padding()
            .navigationTitle("Pair a Device")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("code.done")
                }
            }
            .forgeAlert($model.alert) { action in Task { await model.handle(action) } }
        }
        .onAppear {
            pairedBefore = model.peers.count
            model.makeInvitation(hosts: RelayNetwork.localHosts())
        }
        .onDisappear { model.cancelInvitation() }
        // The other device paired: Rust saved it, and the list grew.
        .onChange(of: model.peers.count) {
            if model.peers.count > pairedBefore {
                model.invitationUsed()
                dismiss()
            }
        }
    }

    static func qrCode(_ text: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 10, y: 10)),
              let image = CIContext().createCGImage(output, from: output.extent)
        else { return nil }
        return UIImage(cgImage: image)
    }
}

/// Scans another device's pairing code with the camera (VisionKit), or
/// takes one pasted as text, and pairs.
struct ScanCodeSheet: View {
    let model: RelayModel
    @Environment(\.dismiss) private var dismiss
    @State private var pasted = ""
    @State private var problem: String?
    @State private var pairing = false

    private var canScan: Bool {
        DataScannerViewController.isSupported && DataScannerViewController.isAvailable
    }

    var body: some View {
        NavigationStack {
            List {
                if canScan {
                    Section {
                        CodeScanner { code in pair(code) }
                            .frame(height: 300)
                            .listRowInsets(EdgeInsets())
                            .accessibilityLabel("Camera, looking for a pairing code")
                            .accessibilityIdentifier("scan.camera")
                    } footer: {
                        Text("Point the camera at the code on the other device.")
                    }
                }
                Section {
                    TextField("forgerelay://pair?…", text: $pasted, axis: .vertical)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("scan.paste")
                    Button("Pair") { pair(pasted) }
                        .disabled(pairing || pasted.isEmpty)
                        .accessibilityIdentifier("scan.pair")
                } header: {
                    Text(canScan ? "Or Paste a Code" : "Paste a Code")
                }
                if pairing {
                    Section { ProgressView("Pairing…") }
                }
                if let problem {
                    Section {
                        Text(problem).forgeForeground(.danger)
                    }
                }
            }
            .navigationTitle("Scan a Code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("scan.cancel")
                }
            }
        }
    }

    private func pair(_ code: String) {
        guard !pairing else { return }
        pairing = true
        problem = nil
        Task {
            defer { pairing = false }
            do {
                _ = try await model.pair(code: code)
                dismiss()
            } catch let error as RelayError {
                problem = RelayModel.describe(error)
            } catch {
                problem = error.localizedDescription
            }
        }
    }
}

/// VisionKit's live scanner, reporting the first Relay code it sees.
private struct CodeScanner: UIViewControllerRepresentable {
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
            // Checked as available just before; pasting a code still works.
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
                      let text = code.payloadStringValue,
                      text.hasPrefix("forgerelay://"),
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
