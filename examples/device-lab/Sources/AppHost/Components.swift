@preconcurrency import ForgeFFI
import SwiftUI

/// A small line chart of recent values, scaled to its own range.
struct Sparkline: View {
    let values: [Double]
    var color: Color = .accentColor

    var body: some View {
        GeometryReader { geometry in
            if values.count > 1, let low = values.min(), let high = values.max() {
                let span = max(high - low, 1e-9)
                let step = geometry.size.width / CGFloat(values.count - 1)
                Path { path in
                    for (index, value) in values.enumerated() {
                        let point = CGPoint(
                            x: CGFloat(index) * step,
                            y: geometry.size.height * (1 - CGFloat((value - low) / span)))
                        if index == 0 { path.move(to: point) } else { path.addLine(to: point) }
                    }
                }
                .stroke(color, style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
            }
        }
        .accessibilityHidden(true)
    }
}

/// A value with its label, a live sparkline under it.
struct ReadingRow: View {
    let title: String
    let symbol: String
    let value: String
    var detail: String? = nil
    var history: [Double] = []
    var color: Color = .accentColor

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            LabeledContent {
                Text(value).monospacedDigit()
            } label: {
                Label(title, systemImage: symbol)
            }
            if !history.isEmpty {
                Sparkline(values: history, color: color)
                    .frame(height: 28)
            }
            if let detail {
                Text(detail)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// What a permission-gated section shows before it can run: a button the
/// first time, the way to Settings after a refusal.
struct PermissionPrompt: View {
    let status: PermissionStatus
    let allowTitle: String
    let explanation: String
    let request: @MainActor () async -> Void

    var body: some View {
        switch status {
        case .notDetermined:
            VStack(alignment: .leading, spacing: 8) {
                Text(explanation).font(.subheadline).foregroundStyle(.secondary)
                Button(allowTitle) { Task { await request() } }
            }
        case .denied:
            VStack(alignment: .leading, spacing: 8) {
                Text("Turned off for Device Lab.").foregroundStyle(.secondary)
                OpenSettingsButton()
            }
        case .restricted:
            Text("Restricted on this device (Screen Time or device management).")
                .foregroundStyle(.secondary)
        case .unsupported:
            Text("Not available here.").foregroundStyle(.secondary)
        case .granted, .provisional:
            EmptyView()
        }
    }
}

struct OpenSettingsButton: View {
    var body: some View {
        #if os(iOS)
            Button("Open Settings") { ForgePermissions.openSettings() }
        #else
            EmptyView()
        #endif
    }
}

/// Grey explanatory text in a row.
struct Note: View {
    let text: String

    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text).font(.footnote).foregroundStyle(.secondary)
    }
}

// The package also builds for macOS (`swift test` runs the model tests on a
// Mac host), so iOS-only modifiers are wrapped here.
extension View {
    func groupedList() -> some View {
        #if os(iOS)
            listStyle(.insetGrouped)
        #else
            self
        #endif
    }

    func inlineNavigationTitle() -> some View {
        #if os(iOS)
            navigationBarTitleDisplayMode(.inline)
        #else
            self
        #endif
    }
}
