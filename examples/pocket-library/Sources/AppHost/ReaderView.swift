@preconcurrency import AVFoundation
@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Pushed from a book's detail screen to open it in the reader.
struct ReaderRoute: Hashable {
    let bookID: UInt64
}

/// Reads a book's EPUB: one chapter at a time, scrolling, with the book's
/// text laid out natively (so it follows Dynamic Type, dark mode and the
/// reader's own text settings). Rust parses the EPUB and decides what the
/// position means for the book's page; this view only lays out blocks and
/// reports which one is at the top.
struct ReaderView: View {
    let bookID: UInt64
    @Environment(LibraryModel.self) private var model
    @Environment(\.dynamicTypeSize) private var systemTextSize
    /// Steps larger or smaller than the system text size.
    @AppStorage("readerTextSizeStep") private var textStep = 0
    @AppStorage("readerSerif") private var serif = true

    @State private var contents: ReaderContents?
    @State private var chapter: ReaderChapter?
    @State private var openError: ForgeFailure?
    @State private var error: ForgeAlert<Never>?
    /// The block at the top of the screen (the chapter's end view is
    /// `blocks.count`).
    @State private var topBlock: Int?
    /// The block to scroll to once a newly loaded chapter is on screen.
    @State private var pendingScroll: Int?
    @State private var loadingChapter = false
    @State private var showingContents = false
    @State private var showingSummary = false
    @State private var lastSaved: ReadingPosition?
    @State private var finishedCount = 0
    /// Counts chapter loads: each gets a fresh scroll view.
    @State private var loadCount = 0
    @State private var readAloud = ReadAloud()
    @State private var voices: [VoiceCandidate] = []

    var body: some View {
        Group {
            if let openError {
                ContentUnavailableView {
                    Label(openError.title, systemImage: "book.closed")
                } description: {
                    Text(openError.message)
                }
            } else if let contents, let chapter {
                reader(contents, chapter)
            } else {
                ProgressView()
            }
        }
        .navigationTitle(chapter?.title ?? contents?.title ?? "")
        .inlineNavigationTitle()
        .hidesTabBar()
        .toolbar { toolbar }
        .sheet(isPresented: $showingContents) {
            if let contents {
                ReaderContentsSheet(contents: contents, current: chapter?.index ?? 0) { index in
                    Task { await show(chapter: index, fraction: 0) }
                }
                .presentationDetents([.medium, .large])
            }
        }
        .sheet(isPresented: $showingSummary) {
            if let chapter {
                ChapterSummarySheet(
                    chapterTitle: chapter.title,
                    state: ChapterSummaryState(
                        library: model, bookID: bookID, chapter: chapter.index,
                        model: AppleIntelligenceSummarizer())
                )
                .presentationDetents([.medium, .large])
            }
        }
        .forgeAlert($error)
        .sensoryFeedback(.success, trigger: finishedCount)
        .task { await open() }
        // Save a moment after scrolling stops, when leaving, and when the
        // app goes to the background.
        .task(id: PositionKey(chapter: chapter?.index, block: topBlock)) {
            do { try await Task.sleep(for: .seconds(1.5)) } catch { return }
            savePosition()
        }
        .forgeSaveOnLeave { savePosition() }
        .onDisappear {
            readAloud.stop()
            readAloud.removeRemoteCommands()
        }
    }

    // MARK: Layout

    private func reader(_ contents: ReaderContents, _ chapter: ReaderChapter) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(Array(chapter.blocks.enumerated()), id: \.offset) { index, block in
                        ReaderBlockView(
                            block: block,
                            serif: serif,
                            spokenWord: readAloud.block == index ? readAloud.word : nil
                        )
                        .id(index)
                    }
                    chapterEnd(contents, chapter)
                        .id(chapter.blocks.count)
                }
                .scrollTargetLayout()
                .padding()
                .frame(maxWidth: 640)
                .frame(maxWidth: .infinity)
                .dynamicTypeSize(textSize)
            }
            .scrollPosition(id: $topBlock, anchor: .top)
            .task(id: loadCount) {
                // A new chapter starts at the top; a restored one at the
                // saved block, once its rows exist.
                guard let target = pendingScroll else { return }
                if target > 0 {
                    try? await Task.sleep(for: .milliseconds(50))
                    proxy.scrollTo(target, anchor: .top)
                }
                topBlock = target
                pendingScroll = nil
            }
            // Follow the voice: the block being read goes to the top.
            .onChange(of: readAloud.block) { _, block in
                guard let block, readAloud.state == .playing, pendingScroll == nil else { return }
                withAnimation { proxy.scrollTo(block, anchor: .top) }
            }
        }
        // A fresh scroll view per chapter, so each starts at its top.
        .id(loadCount)
        .safeAreaInset(edge: .bottom) {
            VStack(spacing: 0) {
                if readAloud.state != .stopped {
                    player
                    Divider()
                }
                progressBar(contents, chapter)
            }
            .background(.bar)
        }
    }

    @ViewBuilder
    private func chapterEnd(_ contents: ReaderContents, _ chapter: ReaderChapter) -> some View {
        VStack(spacing: 12) {
            Divider()
            if let next = contents.chapters.first(where: { $0.index == chapter.index + 1 }) {
                Button {
                    Task { await show(chapter: next.index, fraction: 0) }
                } label: {
                    Label("Next: \(next.title)", systemImage: "chevron.forward")
                        .multilineTextAlignment(.center)
                }
                .buttonStyle(.borderedProminent)
                .disabled(loadingChapter)
                .accessibilityIdentifier("reader.nextChapterEnd")
            } else {
                Text("The End")
                    .font(.headline)
                if let book = model.book(id: bookID), book.status != .finished {
                    Button {
                        markFinished()
                    } label: {
                        Label("Mark as Finished", systemImage: "checkmark.circle")
                    }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("reader.markFinished")
                }
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 24)
    }

    /// "Chapter 8 of 50 · 23%", with previous and next.
    private func progressBar(_ contents: ReaderContents, _ chapter: ReaderChapter) -> some View {
        let count = contents.chapters.count
        let index = Int(chapter.index)
        return HStack {
            Button("Previous Chapter", systemImage: "chevron.backward") {
                Task { await show(chapter: chapter.index - 1, fraction: 0) }
            }
            .labelStyle(.iconOnly)
            .disabled(index == 0 || loadingChapter)
            .accessibilityIdentifier("reader.previousChapter")
            Spacer()
            VStack(spacing: 2) {
                ProgressView(value: bookFraction(contents, chapter))
                Text("Chapter \(index + 1) of \(count) · \(Int((bookFraction(contents, chapter) * 100).rounded()))%")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            .frame(maxWidth: 280)
            .accessibilityElement(children: .combine)
            Spacer()
            Button("Next Chapter", systemImage: "chevron.forward") {
                Task { await show(chapter: chapter.index + 1, fraction: 0) }
            }
            .labelStyle(.iconOnly)
            .disabled(index + 1 >= count || loadingChapter)
            .accessibilityIdentifier("reader.nextChapter")
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }

    /// Read-aloud controls: previous and next paragraph, play or pause,
    /// speed and voice, stop.
    private var player: some View {
        HStack(spacing: 28) {
            Button("Previous Paragraph", systemImage: "backward.fill") { readAloud.skip(by: -1) }
            Button(
                readAloud.state == .playing ? "Pause" : "Play",
                systemImage: readAloud.state == .playing ? "pause.fill" : "play.fill"
            ) {
                readAloud.toggle(from: topBlock ?? 0)
            }
            .font(.title2)
            Button("Next Paragraph", systemImage: "forward.fill") { readAloud.skip(by: 1) }
            Menu {
                Picker("Speed", selection: $readAloud.speed) {
                    ForEach(Self.speeds, id: \.self) { speed in
                        Text(Self.speedLabel(speed)).tag(speed)
                    }
                }
                Picker("Voice", selection: $readAloud.voiceID) {
                    Text("Best Available").tag(String?.none)
                    ForEach(voices) { voice in
                        Text(VoiceChoice.label(for: voice)).tag(String?.some(voice.id))
                    }
                }
            } label: {
                Text(Self.speedLabel(readAloud.speed))
                    .monospacedDigit()
                    .accessibilityLabel("Speed and voice")
            }
            Button("Stop Reading Aloud", systemImage: "xmark") { readAloud.stop() }
        }
        .labelStyle(.iconOnly)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity)
    }

    private static let speeds: [Double] = [0.75, 1, 1.25, 1.5, 1.75, 2, 2.5, 3]

    private static func speedLabel(_ speed: Double) -> String {
        speed.formatted(.number.precision(.fractionLength(0...2))) + "×"
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Menu {
                Button("Larger Text", systemImage: "textformat.size.larger") { textStep += 1 }
                    .disabled(textSize == DynamicTypeSize.allCases.last)
                Button("Smaller Text", systemImage: "textformat.size.smaller") { textStep -= 1 }
                    .disabled(textSize == DynamicTypeSize.allCases.first)
                if textStep != 0 {
                    Button("System Text Size", systemImage: "arrow.counterclockwise") { textStep = 0 }
                }
                Picker("Font", selection: $serif) {
                    Text("Serif").tag(true)
                    Text("System").tag(false)
                }
            } label: {
                Label("Text Settings", systemImage: "textformat.size")
            }
            .accessibilityIdentifier("reader.textSettings")
        }
        ToolbarItem(placement: .primaryAction) {
            Button("Contents", systemImage: "list.bullet") { showingContents = true }
                .disabled(contents == nil)
                .accessibilityIdentifier("reader.contents")
        }
        ToolbarItem(placement: .primaryAction) {
            Button("Summarise Chapter", systemImage: "sparkles") { showingSummary = true }
                .disabled(chapter == nil)
                .accessibilityIdentifier("reader.summary")
        }
        ToolbarItem(placement: .primaryAction) {
            Button("Read Aloud", systemImage: "headphones") {
                readAloud.play(from: topBlock ?? 0)
            }
            .disabled(chapter == nil || readAloud.state != .stopped)
            .accessibilityIdentifier("reader.readAloud")
        }
    }

    private var textSize: DynamicTypeSize {
        let sizes = DynamicTypeSize.allCases
        let base = sizes.firstIndex(of: systemTextSize) ?? sizes.startIndex
        return sizes[min(max(base + textStep, 0), sizes.count - 1)]
    }

    /// How far through the book the top of the screen is, by words (for
    /// the progress bar only; Rust works out the page).
    private func bookFraction(_ contents: ReaderContents, _ chapter: ReaderChapter) -> Double {
        guard contents.totalWords > 0 else { return 0 }
        let before = contents.chapters.prefix(Int(chapter.index)).reduce(0.0) { $0 + Double($1.words) }
        let here = Double(contents.chapters[Int(chapter.index)].words) * fraction(in: chapter)
        return min((before + here) / Double(contents.totalWords), 1)
    }

    private func fraction(in chapter: ReaderChapter) -> Double {
        guard !chapter.blocks.isEmpty else { return 0 }
        return min(Double(topBlock ?? 0) / Double(chapter.blocks.count), 1)
    }

    // MARK: Actions

    private func open() async {
        guard contents == nil else { return }
        do {
            let contents = try await model.openReader(bookID)
            self.contents = contents
            lastSaved = contents.start
            prepareReadAloud(contents)
            await show(chapter: contents.start.chapter, fraction: contents.start.fraction)
        } catch {
            openError = ForgeFailure(error)
        }
    }

    private func show(chapter index: UInt32, fraction: Double) async {
        savePosition()
        loadingChapter = true
        defer { loadingChapter = false }
        do {
            let loaded = try await model.chapter(bookID, index: index)
            let last = max(loaded.blocks.count - 1, 0)
            pendingScroll = min(Int((fraction * Double(loaded.blocks.count)).rounded(.down)), last)
            topBlock = nil
            chapter = loaded
            loadCount += 1
            // Reading aloud carries on into the new chapter.
            let wasPlaying = readAloud.state == .playing
            if readAloud.state == .paused { readAloud.stop() }
            readAloud.load(loaded)
            if wasPlaying { readAloud.play(from: pendingScroll ?? 0) }
        } catch {
            if contents == nil || chapter == nil {
                openError = ForgeFailure(error)
            } else {
                self.error = .failure(error)
            }
        }
    }

    private func prepareReadAloud(_ contents: ReaderContents) {
        let book = model.book(id: bookID)
        readAloud.setBook(title: contents.title, author: book?.author ?? "", coverFile: book?.coverFile)
        voices = ReadAloud.voices()
        readAloud.onBlock = { block in
            // Also runs with the screen locked, so save from here rather
            // than from the scroll position.
            topBlock = block
            savePosition(block: block)
        }
        readAloud.onChapterEnd = {
            Task {
                guard let chapter, let contents = self.contents,
                      Int(chapter.index) + 1 < contents.chapters.count else {
                    readAloud.stop()
                    return
                }
                await show(chapter: chapter.index + 1, fraction: 0)
            }
        }
    }

    private func savePosition(block: Int? = nil) {
        guard let chapter, pendingScroll == nil else { return }
        let fraction = block.map { chapter.blocks.isEmpty ? 0 : min(Double($0) / Double(chapter.blocks.count), 1) }
            ?? fraction(in: chapter)
        let position = ReadingPosition(chapter: chapter.index, fraction: fraction)
        guard position != lastSaved else { return }
        let previous = lastSaved
        lastSaved = position
        Task {
            do {
                try await model.saveReadingPosition(bookID, chapter: position.chapter, fraction: position.fraction)
            } catch {
                // Not saved: the next move, or leaving, tries again.
                if lastSaved == position { lastSaved = previous }
                self.error = .failure(error)
            }
        }
    }

    private func markFinished() {
        Task {
            do {
                try await model.setStatus(bookID, .finished)
                finishedCount += 1
            } catch {
                self.error = .failure(error)
            }
        }
    }
}

private struct PositionKey: Hashable {
    let chapter: UInt32?
    let block: Int?
}

/// One paragraph, heading, quote, list item, scene break or image.
struct ReaderBlockView: View {
    let block: TextBlock
    let serif: Bool
    /// The word being read aloud, if this block is being read.
    var spokenWord: NSRange?

    private var design: Font.Design { serif ? .serif : .default }

    var body: some View {
        switch block.kind {
        case .heading(let level):
            Text(attributed)
                .font(.system(headingStyle(level), design: design, weight: .semibold))
                .padding(.top, level <= 2 ? 16 : 8)
                .accessibilityAddTraits(.isHeader)
        case .paragraph:
            Text(attributed)
                .font(.system(.body, design: design))
                .lineSpacing(4)
        case .quote:
            Text(attributed)
                .font(.system(.body, design: design))
                .lineSpacing(4)
                .foregroundStyle(.secondary)
                .padding(.leading, 16)
                .overlay(alignment: .leading) {
                    Capsule()
                        .fill(.quaternary)
                        .frame(width: 3)
                }
        case .listItem(let marker):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(marker)
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                Text(attributed)
                    .lineSpacing(4)
            }
            .font(.system(.body, design: design))
        case .preformatted:
            ScrollView(.horizontal) {
                Text(attributed)
                    .font(.system(.callout, design: .monospaced))
            }
        case .separator:
            Text("⁂")
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity)
                .accessibilityLabel("Section break")
        case .image(let file, let alt):
            ReaderImage(file: file, alt: alt)
        }
    }

    private func headingStyle(_ level: UInt8) -> Font.TextStyle {
        switch level {
        case 1: return .title
        case 2: return .title2
        case 3: return .title3
        default: return .headline
        }
    }

    private var attributed: AttributedString {
        var text = AttributedString()
        for span in block.spans {
            var part = AttributedString(span.text)
            var intent: InlinePresentationIntent = []
            if span.bold { intent.insert(.stronglyEmphasized) }
            if span.italic { intent.insert(.emphasized) }
            if span.monospace { intent.insert(.code) }
            if !intent.isEmpty { part.inlinePresentationIntent = intent }
            text.append(part)
        }
        if let spokenWord, let range = Range<AttributedString.Index>(spokenWord, in: text) {
            text[range].backgroundColor = Color.accentColor.opacity(0.3)
        }
        return text
    }
}

/// A picture from the book, scaled to the text column.
private struct ReaderImage: View {
    let file: String
    let alt: String
    @State private var image: Image?
    @State private var loaded = false

    var body: some View {
        Group {
            if let image {
                image
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity)
            } else if !loaded {
                ProgressView()
                    .frame(maxWidth: .infinity, minHeight: 120)
            }
        }
        .accessibilityElement()
        .accessibilityLabel(alt.isEmpty ? "Picture" : alt)
        .accessibilityHidden(alt.isEmpty)
        .task(id: file) {
            image = await CoverThumbnails.image(at: file, maxPixels: 1600)
            loaded = true
        }
    }
}

/// The book's table of contents; picking a chapter opens it.
private struct ReaderContentsSheet: View {
    let contents: ReaderContents
    let current: UInt32
    let onSelect: (UInt32) -> Void
    @Environment(\.dismiss) private var dismiss

    /// The chapters the book lists, or every chapter if it lists none.
    private var entries: [ChapterSummary] {
        let listed = contents.chapters.filter(\.inContents)
        return listed.isEmpty ? contents.chapters : listed
    }

    /// The entry the current chapter belongs to.
    private var currentEntry: UInt32? {
        entries.last(where: { $0.index <= current })?.index
    }

    var body: some View {
        NavigationStack {
            ScrollViewReader { proxy in
                List(entries, id: \.index) { entry in
                    Button {
                        onSelect(entry.index)
                        dismiss()
                    } label: {
                        HStack {
                            Text(entry.title)
                                .foregroundStyle(.primary)
                            Spacer()
                            if entry.index == currentEntry {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(.tint)
                                    .accessibilityLabel("Current chapter")
                            }
                        }
                    }
                    .accessibilityIdentifier("contents.chapter.\(entry.index)")
                }
                .onAppear {
                    if let currentEntry { proxy.scrollTo(currentEntry, anchor: .center) }
                }
            }
            .navigationTitle("Contents")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("contents.done")
                }
            }
        }
    }
}
