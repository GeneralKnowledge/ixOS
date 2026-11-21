#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// Shows the system share sheet (AirDrop, Save to Files, Mail, Messages)
    /// for `file` as soon as the app sets it, and sets it back to nil when
    /// the sheet closes. For a file the app has just written, an export:
    ///
    ///     @State private var exported: URL?
    ///     Button("Export as GPX") { Task { exported = try? await model.export(trip) } }
    ///     ...
    ///     .forgeShareFile($exported)
    ///
    /// There is no "your file is ready" step in between: the person asked
    /// for the export, so the share sheet is what they see next. Show a
    /// failed export as you would any failed action; set `file` only once
    /// the file is written. A share button for something already there
    /// (a link, a book's details) stays a `ShareLink`.
    public func forgeShareFile(_ file: Binding<URL?>) -> some View {
        #if canImport(UIKit)
            sheet(
                item: Binding(
                    get: { file.wrappedValue.map(ForgeSharedFile.init) },
                    set: { file.wrappedValue = $0?.url })
            ) { shared in
                ForgeActivityView(items: [shared.url]) { file.wrappedValue = nil }
                    .ignoresSafeArea()
                    .presentationDetents([.medium, .large])
            }
        #else
            self
        #endif
    }
}

/// A file for `sheet(item:)`.
struct ForgeSharedFile: Identifiable {
    let url: URL
    var id: URL { url }
}

#if canImport(UIKit)
import UIKit

/// The system share sheet. SwiftUI's `ShareLink` needs a tap of its own;
/// this shows the same sheet straight away.
struct ForgeActivityView: UIViewControllerRepresentable {
    let items: [Any]
    let finished: () -> Void

    func makeUIViewController(context: Context) -> UIActivityViewController {
        let controller = UIActivityViewController(activityItems: items, applicationActivities: nil)
        controller.completionWithItemsHandler = { _, _, _, _ in finished() }
        return controller
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
#endif
#endif
