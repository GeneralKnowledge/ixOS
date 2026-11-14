@preconcurrency import PocketLibraryFFI
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

/// The whole book, read by Apple Intelligence on the iPhone: an overview,
/// who's who, and every chapter's summary. While it runs (minutes for a
/// novel), shows the chapter, which role is working, the text being
/// written and the time so far, and keeps the screen awake.
struct BookSummarySheet: View {
    let bookTitle: String
    @State var state: BookSummaryState
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    content
                }
                .padding()
                .frame(maxWidth: 640, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .navigationTitle(bookTitle)
            .inlineNavigationTitle()
            .toolbar { toolbar }
        }
        .onAppear { state.load() }
        .onDisappear {
            state.stop()
            keepAwake(false)
        }
        .onChange(of: state.phase) { _, phase in keepAwake(phase == .working) }
        // Stopped in the background: carry on from the saved progress.
        .onChange(of: scenePhase) { _, phase in
            if phase == .active, state.pausedInBackground { state.start() }
        }
        .interactiveDismissDisabled(state.phase == .working)
    }

    @ViewBuilder
    private var content: some View {
        switch state.phase {
        case .loading:
            ProgressView().frame(maxWidth: .infinity)
        case .empty:
            if state.info.available { offer } else { unavailable }
        case .working:
            working
        case .ready:
            if let synopsis = state.synopsis { result(synopsis) }
        case .failed:
            failed
        }
    }

    private var offer: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Apple Intelligence reads the whole book on this iPhone, chapter by chapter, and writes an overview, a who's who and a summary of every chapter.")
            Text("A team of roles takes turns on the phone's model: a reader lists what happens in each part, a writer summarises each chapter, an editor checks it doesn't copy the book, and a cast keeper follows the people. A novel can take 10 to 20 minutes. You can leave Pocket Library: on iOS 26 it carries on in the background, with its progress on the Lock Screen, for as long as iOS allows. Progress is saved after every chapter, so a stopped run picks up where it left off.")
                .font(.footnote)
                .foregroundStyle(.secondary)
            Label("The overview tells you how the book ends.", systemImage: "eye.trianglebadge.exclamationmark")
                .font(.footnote)
                .foregroundStyle(.secondary)
            Button {
                state.start()
            } label: {
                Label(startTitle, systemImage: "sparkles")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .accessibilityIdentifier("bookSummary.start")
        }
    }

    private var startTitle: String {
        guard let run = state.unfinished else { return "Read the Whole Book" }
        return "Carry On from Chapter \(run.chaptersDone + 1) of \(run.chapters)"
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
                Text(state.headline)
                    .font(.headline)
            } currentValueLabel: {
                Text(state.detail)
            }
            .animation(.default, value: state.fractionDone)
            if let startedAt = state.startedAt {
                TimelineView(.periodic(from: startedAt, by: 1)) { context in
                    Text("\(elapsed(since: startedAt, now: context.date)) so far")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
            if let note = state.backgroundNote {
                Label("Keep Pocket Library open: it can't read in the background here. \(note).", systemImage: "info.circle")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            if !state.draft.isEmpty {
                Text(state.draft)
                    .font(.system(.body, design: .serif))
                    .lineSpacing(4)
                    .foregroundStyle(.secondary)
            }
            Button("Stop", role: .cancel) { state.stop() }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("bookSummary.stop")
        }
        .accessibilityElement(children: .contain)
    }

    private func result(_ synopsis: BookSynopsis) -> some View {
        VStack(alignment: .leading, spacing: 20) {
            Text(synopsis.text)
                .font(.system(.body, design: .serif))
                .lineSpacing(4)
                .textSelection(.enabled)
                .accessibilityIdentifier("bookSummary.text")
            if !synopsis.cast.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Who's Who")
                        .font(.title3.weight(.semibold))
                        .accessibilityAddTraits(.isHeader)
                    ForEach(synopsis.cast, id: \.name) { person in
                        Text("\(Text(person.name).bold()) · \(person.role)")
                            .font(.callout)
                    }
                }
            }
            if !synopsis.chapters.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Chapter by Chapter")
                        .font(.title3.weight(.semibold))
                        .accessibilityAddTraits(.isHeader)
                    ForEach(synopsis.chapters, id: \.chapter) { chapter in
                        DisclosureGroup(chapter.title) {
                            Text(chapter.text)
                                .font(.system(.callout, design: .serif))
                                .textSelection(.enabled)
                                .padding(.vertical, 4)
                        }
                        .accessibilityIdentifier("bookSummary.chapter.\(chapter.chapter)")
                    }
                }
            }
            Text(footer(synopsis))
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private var failed: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let error = state.error {
                Text(ErrorText.title(error)).font(.headline)
                Text(ErrorText.message(error))
            }
            Text(state.pausedInBackground
                ? "It stopped while Pocket Library was in the background. It carries on from the last chapter when you come back."
                : "Progress is saved after every chapter, so trying again carries on from there.")
                .font(.footnote)
                .foregroundStyle(.secondary)
            if state.info.available {
                Button("Carry On") { state.start() }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("bookSummary.retry")
            }
        }
    }

    private func footer(_ synopsis: BookSynopsis) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(synopsis.createdAtMs) / 1000)
            .formatted(date: .abbreviated, time: .shortened)
        var text = "\(synopsis.modelName) read \(synopsis.chaptersRead) chapters on this iPhone in \(Self.duration(Int(synopsis.seconds))), with \(synopsis.modelRequests) requests to the model, \(date)."
        if synopsis.chaptersSkipped > 0 {
            text += " It declined \(synopsis.chaptersSkipped) chapter\(synopsis.chaptersSkipped == 1 ? "" : "s"), which the summary leaves out."
        }
        return text + " Summaries can get details wrong."
    }

    private func elapsed(since start: Date, now: Date) -> String {
        Self.duration(Int(now.timeIntervalSince(start)))
    }

    static func duration(_ seconds: Int) -> String {
        let (m, s) = (seconds / 60, seconds % 60)
        return m > 0 ? "\(m) min \(s) s" : "\(s) s"
    }

    private func keepAwake(_ on: Bool) {
        #if canImport(UIKit) && !os(macOS)
        UIApplication.shared.isIdleTimerDisabled = on
        #endif
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .confirmationAction) {
            Button("Done") { dismiss() }
                .disabled(state.phase == .working)
                .accessibilityIdentifier("bookSummary.done")
        }
        if state.phase == .ready, state.info.available {
            ToolbarItem(placement: .primaryAction) {
                Button("Read Again", systemImage: "arrow.clockwise") { state.start() }
                    .accessibilityIdentifier("bookSummary.again")
            }
        }
    }
}
