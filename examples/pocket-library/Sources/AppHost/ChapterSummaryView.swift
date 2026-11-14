@preconcurrency import PocketLibraryFFI
import SwiftUI

/// The reader's chapter summary, written on the iPhone by Apple
/// Intelligence. Shows the saved summary, or offers to write one; while it
/// is written, says which part of the chapter the model is reading, then
/// shows the summary as it appears.
struct ChapterSummarySheet: View {
    let chapterTitle: String
    @State var state: ChapterSummaryState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(chapterTitle)
                        .font(.headline)
                        .foregroundStyle(.secondary)
                    content
                }
                .padding()
                .frame(maxWidth: 640, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .navigationTitle("Chapter Summary")
            .inlineNavigationTitle()
            .toolbar { toolbar }
        }
        .task { await state.load() }
        .onDisappear { state.stop() }
    }

    @ViewBuilder
    private var content: some View {
        switch state.phase {
        case .loading:
            ProgressView()
                .frame(maxWidth: .infinity)
        case .empty:
            if state.info.available {
                offer
            } else {
                unavailable
            }
        case .working:
            working
        case .ready:
            if let synopsis = state.synopsis {
                summary(synopsis)
            }
        case .failed:
            failed
        }
    }

    private var offer: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Pocket Library can summarise this chapter with Apple Intelligence. It runs on this iPhone, so the book never leaves it.")
            Text("Long chapters are read a part at a time, which can take a minute.")
                .font(.footnote)
                .foregroundStyle(.secondary)
            Button {
                state.summarise()
            } label: {
                Label("Summarise Chapter", systemImage: "sparkles")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .accessibilityIdentifier("summary.start")
        }
    }

    private var unavailable: some View {
        ContentUnavailableView {
            Label("Summaries Unavailable", systemImage: "sparkles")
        } description: {
            Text(state.info.unavailableReason ?? "Apple Intelligence is not available.")
        }
    }

    private var working: some View {
        VStack(alignment: .leading, spacing: 12) {
            ProgressView(value: state.fractionDone) {
                Text(state.stageText)
                    .font(.subheadline)
            }
            .animation(.default, value: state.fractionDone)
            if !state.draft.isEmpty {
                Text(state.draft)
                    .font(.system(.body, design: .serif))
                    .lineSpacing(4)
            }
            Button("Stop", role: .cancel) { state.stop() }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("summary.stop")
        }
        .accessibilityElement(children: .contain)
    }

    private func summary(_ synopsis: ChapterSynopsis) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(synopsis.text)
                .font(.system(.body, design: .serif))
                .lineSpacing(4)
                .textSelection(.enabled)
                .accessibilityIdentifier("summary.text")
            if synopsis.skippedPassages > 0 {
                Label(
                    "Apple Intelligence declined to read \(synopsis.skippedPassages == 1 ? "a passage" : "\(synopsis.skippedPassages) passages") of this chapter, so the summary may leave something out.",
                    systemImage: "exclamationmark.triangle"
                )
                .font(.footnote)
                .foregroundStyle(.secondary)
            }
            Text(footer(synopsis))
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private var failed: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let error = state.error {
                Text(ErrorText.title(error))
                    .font(.headline)
                Text(ErrorText.message(error))
            }
            if state.info.available {
                Button("Try Again") { state.summarise() }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("summary.retry")
            }
        }
    }

    private func footer(_ synopsis: ChapterSynopsis) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(synopsis.createdAtMs) / 1000)
            .formatted(date: .abbreviated, time: .shortened)
        let parts = synopsis.parts > 1 ? " Read in \(synopsis.parts) parts." : ""
        return "Written by \(synopsis.modelName) on this iPhone, \(date).\(parts) Summaries can get details wrong."
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .confirmationAction) {
            Button("Done") { dismiss() }
                .accessibilityIdentifier("summary.done")
        }
        if state.phase == .ready, state.info.available {
            ToolbarItem(placement: .primaryAction) {
                Button("Summarise Again", systemImage: "arrow.clockwise") { state.summarise() }
                    .accessibilityIdentifier("summary.again")
            }
        }
    }
}
