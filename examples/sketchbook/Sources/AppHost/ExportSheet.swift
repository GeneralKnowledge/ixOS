#if os(iOS)
@preconcurrency import SketchbookFFI
import ForgeAppKit
import Photos
import QuickLook
import SwiftUI
import UIKit
import ForgeUI

/// What an export made: a look at it, then Save to Photos (images), Share,
/// or open it in Quick Look.
struct ExportSheet: View {
    let export: SketchExport
    @Environment(\.dismiss) private var dismiss
    @State private var saving = false
    @State private var saved = false
    @State private var feedback = ForgeFeedback()
    @State private var problem: String?
    @State private var preview: URL?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    if let image = export.image {
                        Image(uiImage: image)
                            .resizable()
                            .scaledToFit()
                            .frame(maxHeight: 260)
                            .frame(maxWidth: .infinity)
                            .accessibilityLabel("The exported sketch")
                    }
                    LabeledContent("File", value: export.url.lastPathComponent)
                }
                Section {
                    if export.format == .png {
                        Button {
                            save()
                        } label: {
                            Label(saved ? "Saved to Photos" : "Save to Photos",
                                  systemImage: saved ? "checkmark.circle" : "photo.badge.plus")
                        }
                        .disabled(saving || saved)
                        .accessibilityIdentifier("export.savePhotos")
                    }
                    ShareLink(item: export.url) {
                        Label(export.format == .png ? "Share Image" : "Share PDF", systemImage: "square.and.arrow.up")
                    }
                    .accessibilityIdentifier("export.share")
                    Button {
                        preview = export.url
                    } label: {
                        Label("Open in Quick Look", systemImage: "eye")
                    }
                    .accessibilityIdentifier("export.preview")
                }
                if let problem {
                    Section {
                        Text(problem).forgeForeground(.danger)
                    }
                }
            }
            .navigationTitle(export.format == .png ? "Image" : "PDF")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("export.done")
                }
            }
            .quickLookPreview($preview)
            .forgeFeedback(feedback)
        }
        .presentationDetents([.medium, .large])
    }

    /// Adds the PNG to the photo library. Asks for add-only access, which
    /// can't read the library.
    private func save() {
        saving = true
        problem = nil
        let url = export.url
        Task {
            defer { saving = false }
            let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
            guard status == .authorized || status == .limited else {
                problem = "Sketchbook can't add to Photos. Allow it in Settings, then try again."
                return
            }
            do {
                try await PHPhotoLibrary.shared().performChanges {
                    _ = PHAssetChangeRequest.creationRequestForAssetFromImage(atFileURL: url)
                }
                saved = true
                feedback.play(.success)
            } catch {
                problem = "Couldn't save to Photos: \(error.localizedDescription)"
            }
        }
    }
}

/// A finished export: the file, and a picture of its first page.
struct SketchExport: Identifiable {
    let url: URL
    let format: ExportFormat
    let image: UIImage?
    var id: URL { url }
}
#endif
