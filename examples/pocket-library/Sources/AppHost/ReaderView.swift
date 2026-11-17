@preconcurrency import AVFoundation
@preconcurrency import PocketLibraryFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Reads a book's EPUB: one chapter at a time, scrolling, with the book's
/// text laid out natively by ForgeReader (so it follows Dynamic Type, dark
/// mode and the reader's own text settings). Rust parses the EPUB, keeps
/// the place as a block anchor and decides what it means for the book's
/// page; this view reports which block is at the top.
struct ReaderView: View {
    let bookID: UInt64
    @Environment(LibraryModel.self) private var model

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
    @State private var showingBookSummary = false
    @State private var lastSaved: SavedPlace?
    /// Success when the book is marked finished.
    @State private var feedback = ForgeFeedback()
    /// Counts chapter loads: each gets a fresh scroll view.
    @State private var loadCount = 0
    @State private var readAloud = ReadAloud()
    @State private var voices: [VoiceCandidate] = []

    var body: some View {
        Group {
            if let openError {
                ForgeStateView.failed(openError.title, message: openError.message)
            } else if let contents, let chapter {
                reader(contents, chapter)
            } else {
                ProgressView()
            }
        }
        .navigationTitle(chapter?.section.title ?? contents?.title ?? "")
        .inlineNavigationTitle()
        .hidesTabBar()
        .toolbar { toolbar }
        .sheet(isPresented: $showingContents) {
            if let contents {
                ReaderContentsSheet(contents: contents, current: chapter?.index ?? 0) { index in
                    Task { await show(chapter: index) }
                }
                .presentationDetents([.medium, .large])
            }
        }
        .sheet(isPresented: $showingSummary) {
            if let chapter {
                ChapterSummarySheet(
                    chapterTitle: chapter.section.title,
                    state: ChapterSummaryState(
                        library: model, bookID: bookID, chapter: chapter.index,
                        model: ForgeAppleModel(permissiveContentTransformations: true))
                )
                .presentationDetents([.medium, .large])
            }
        }
        .sheet(isPresented: $showingBookSummary) {
            BookSummarySheet(
                bookTitle: contents?.title ?? "",
                state: BookSummaryState(
                    library: model, bookID: bookID, bookTitle: contents?.title ?? "",
                    model: ForgeAppleModel(permissiveContentTransformations: true))
            )
        }
        .forgeAlert($error)
        .forgeFeedback(feedback)
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
        ForgeReaderSectionView(
            blocks: chapter.section.blocks,
            topBlock: $topBlock,
            scrollTarget: $pendingScroll,
            spokenBlock: readAloud.block,
            spokenWord: readAloud.word,
            header: { EmptyView() },
            footer: { chapterEnd(contents, chapter) }
        )
        // Follow the voice: the block being read goes to the top.
        .onChange(of: readAloud.block) { _, block in
            guard let block, readAloud.state == .playing, pendingScroll == nil else { return }
            pendingScroll = block
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
                    Task { await show(chapter: next.index) }
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
                Task { await show(chapter: chapter.index - 1) }
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
                Task { await show(chapter: chapter.index + 1) }
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
            ForgeReaderTextMenu(identifier: "reader.textSettings")
        }
        ToolbarItem(placement: .primaryAction) {
            Button("Contents", systemImage: "list.bullet") { showingContents = true }
                .disabled(contents == nil)
                .accessibilityIdentifier("reader.contents")
        }
        ToolbarItem(placement: .primaryAction) {
            Menu {
                Button("Summarise Chapter", systemImage: "text.alignleft") { showingSummary = true }
                    .disabled(chapter == nil)
                    .accessibilityIdentifier("reader.summary.chapter")
                Button("Read the Whole Book", systemImage: "books.vertical") { showingBookSummary = true }
                    .disabled(contents == nil)
                    .accessibilityIdentifier("reader.summary.book")
            } label: {
                Label("Summaries", systemImage: "sparkles")
            }
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

    /// How far through the book the top of the screen is, by words (for
    /// the progress bar only; Rust works out the page).
    private func bookFraction(_ contents: ReaderContents, _ chapter: ReaderChapter) -> Double {
        guard contents.totalWords > 0 else { return 0 }
        let before = contents.chapters.prefix(Int(chapter.index)).reduce(0.0) { $0 + Double($1.words) }
        let here = Double(contents.chapters[Int(chapter.index)].words) * fraction(in: chapter)
        return min((before + here) / Double(contents.totalWords), 1)
    }

    private func fraction(in chapter: ReaderChapter) -> Double {
        let blocks = chapter.section.blocks
        guard !blocks.isEmpty else { return 0 }
        return min(Double(topBlock ?? 0) / Double(blocks.count), 1)
    }

    // MARK: Actions

    private func open() async {
        guard contents == nil else { return }
        do {
            let contents = try await model.openReader(bookID)
            self.contents = contents
            prepareReadAloud(contents)
            await show(chapter: contents.start.chapter)
        } catch {
            openError = ForgeFailure(error)
        }
    }

    /// Loads chapter `index`: at its top, or at the saved place the first
    /// time the chapter the book opens at is loaded (Rust resolves the
    /// saved anchor).
    private func show(chapter index: UInt32) async {
        savePosition()
        loadingChapter = true
        defer { loadingChapter = false }
        do {
            let loaded = try await model.chapter(bookID, index: index)
            let last = max(loaded.section.blocks.count - 1, 0)
            let start = min(Int(loaded.start?.block ?? 0), last)
            pendingScroll = start
            if lastSaved == nil { lastSaved = SavedPlace(chapter: index, block: start) }
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
                await show(chapter: chapter.index + 1)
            }
        }
    }

    private func savePosition(block: Int? = nil) {
        guard let chapter, pendingScroll == nil else { return }
        let place = SavedPlace(chapter: chapter.index, block: block ?? topBlock ?? 0)
        guard place != lastSaved else { return }
        let previous = lastSaved
        lastSaved = place
        Task {
            do {
                try await model.saveReadingPosition(bookID, chapter: place.chapter, block: place.block)
            } catch {
                // Not saved: the next move, or leaving, tries again.
                if lastSaved == place { lastSaved = previous }
                self.error = .failure(error)
            }
        }
    }

    private func markFinished() {
        Task {
            do {
                try await model.setStatus(bookID, .finished)
                feedback.play(.success)
            } catch {
                self.error = .failure(error)
            }
        }
    }
}

/// A block of a chapter: what the reader last saved.
private struct SavedPlace: Equatable {
    let chapter: UInt32
    let block: Int
}

private struct PositionKey: Hashable {
    let chapter: UInt32?
    let block: Int?
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
