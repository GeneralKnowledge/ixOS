@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// Recent log records from Device Lab and every hosted app, kept in memory
/// by Forge's logging pipeline, so what went wrong can be read and shared
/// from the phone without a cable (`forge device logs` still shows them live
/// on a computer).
struct LogsView: View {
    @State private var model = LogsModel()

    var body: some View {
        List(model.shown, id: \.self) { line in
            LogRow(line: line)
        }
        .overlay {
            if model.shown.isEmpty {
                if model.all.isEmpty {
                    ContentUnavailableView(
                        "Nothing Logged Yet", systemImage: "text.alignleft",
                        description: Text("Open an app and use it; what it logs shows here."))
                } else {
                    ContentUnavailableView.search
                }
            }
        }
        .searchable(text: $model.search, prompt: "Message or source")
        .navigationTitle("Logs")
        .inlineNavigationTitle()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Picker("Show", selection: $model.filter) {
                        ForEach(LogsModel.Filter.allCases) { filter in
                            Text(filter.name).tag(filter)
                        }
                    }
                } label: {
                    Label("Filter", systemImage: "line.3.horizontal.decrease.circle")
                }
                .accessibilityIdentifier("logs.filter")
            }
            ToolbarItem(placement: .primaryAction) {
                ShareLink(item: model.shareText) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                .disabled(model.shown.isEmpty)
                .accessibilityIdentifier("logs.share")
            }
        }
        .task { await model.follow() }
    }
}

private struct LogRow: View {
    let line: LogLine

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Image(systemName: line.level.symbol)
                    .forgeForeground(line.level.tone)
                    .accessibilityLabel(line.level.name)
                Text(line.target)
                    .font(.caption.weight(.semibold))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 4)
                Text(Self.time(line.timeMs))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            Text(line.message)
                .font(.footnote.monospaced())
                .textSelection(.enabled)
        }
        .accessibilityElement(children: .combine)
    }

    static func time(_ ms: Int64) -> String {
        Date(timeIntervalSince1970: Double(ms) / 1000)
            .formatted(.dateTime.hour().minute().second().secondFraction(.fractional(3)))
    }
}

private extension LogLevel {
    var name: String {
        switch self {
        case .error: "Error"
        case .warn: "Warning"
        case .info: "Info"
        case .debug: "Debug"
        case .trace: "Trace"
        }
    }

    var symbol: String {
        switch self {
        case .error: "xmark.octagon.fill"
        case .warn: "exclamationmark.triangle.fill"
        case .info: "info.circle"
        case .debug, .trace: "ant"
        }
    }

    var tone: ForgeTone {
        switch self {
        case .error: .danger
        case .warn: .warning
        case .info: .info
        case .debug, .trace: .neutral
        }
    }
}
