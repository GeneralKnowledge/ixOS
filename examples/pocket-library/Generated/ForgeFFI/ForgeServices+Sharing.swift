// Forge native services: the share sheet. Source of truth: crates/forge-services/swift/.

import Foundation
import SwiftUI

/// A share button for a `ShareItem` from Rust: text, a link, or a file the
/// app wrote. The share sheet includes "Save to Files", so it is also how a
/// user exports a file.
public struct ForgeShareLink<Label: View>: View {
    private let item: ShareItem
    private let label: Label

    public init(item: ShareItem, @ViewBuilder label: () -> Label) {
        self.item = item
        self.label = label()
    }

    public var body: some View {
        switch item {
        case .text(let text):
            ShareLink(item: text) { label }
        case .url(let string):
            if let url = URL(string: string) {
                ShareLink(item: url) { label }
            } else {
                ShareLink(item: string) { label }
            }
        case .file(let path, let title):
            ShareLink(item: URL(fileURLWithPath: path), subject: title.map { Text($0) }) { label }
        }
    }
}

extension ForgeShareLink where Label == SwiftUI.Label<Text, Image> {
    /// The standard share button.
    public init(item: ShareItem, title: String = "Share") {
        self.init(item: item) { SwiftUI.Label(title, systemImage: "square.and.arrow.up") }
    }
}
