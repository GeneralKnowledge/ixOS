@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// A2UI Lab's home: the reading plan on stand-in data (A2UI-1), the two app
/// surfaces on real data (C3, C4) and the Bonsai card (C2).
struct A2uiLabHome: View {
    let readingPlan: A2uiLabModel
    @State private var feeds = A2uiFeedsModel(app: A2uiAppHooks.commonplace())
    @State private var playlist = A2uiPlaylistModel(app: A2uiAppHooks.soundscape())
    @State private var card: A2uiTaskCardModel

    init(readingPlan: A2uiLabModel, harness: BonsaiHarness?) {
        self.readingPlan = readingPlan
        _card = State(initialValue: A2uiTaskCardModel(harness: harness))
    }

    var body: some View {
        List {
            Section {
                NavigationLink {
                    A2uiLabView(model: readingPlan)
                } label: {
                    Label("Reading Plan", systemImage: "books.vertical")
                }
                .accessibilityIdentifier("a2uiLab.readingPlan")
            } footer: {
                Text("The first surface, on a stand-in library: no app's books change.")
            }
            Section {
                NavigationLink {
                    A2uiFeedsView(model: feeds)
                } label: {
                    Label("Organise Feeds", systemImage: "folder")
                }
                .accessibilityIdentifier("a2uiLab.feeds")
                NavigationLink {
                    A2uiPlaylistView(model: playlist)
                } label: {
                    Label("Make a Playlist", systemImage: "music.note.list")
                }
                .accessibilityIdentifier("a2uiLab.playlist")
            } header: {
                Text("Your apps")
            } footer: {
                Text("Proposals about Commonplace's feeds and Soundscape's songs. The app checks the plan again and applies it only when you confirm.")
            }
            Section {
                NavigationLink {
                    A2uiTaskCardView(model: card)
                } label: {
                    Label("Bonsai Result", systemImage: "checkmark.seal")
                }
                .accessibilityIdentifier("a2uiLab.taskCard")
            } footer: {
                Text("The last task the Bonsai Harness verified, as a read-only card built from the harness's own evidence.")
            }
        }
        .navigationTitle("A2UI Lab")
        .inlineNavigationTitle()
    }
}

/// The notice at the top of an A2UI screen.
struct A2uiNoticeView: View {
    let notice: A2uiNotice
    let identifier: String

    var body: some View {
        ForgeNotice(notice.text, systemImage: icon, tone: tone)
            .accessibilityIdentifier(identifier)
    }

    private var icon: String {
        switch notice.kind {
        case .info: "info.circle"
        case .refused: "hand.raised"
        case .warning: "exclamationmark.triangle"
        }
    }

    private var tone: ForgeTone {
        switch notice.kind {
        case .info: .info
        case .refused, .warning: .warning
        }
    }
}

/// C3: Commonplace's feeds, with a suggested folder each.
struct A2uiFeedsView: View {
    let model: A2uiFeedsModel
    @StateObject private var host: A2uiSurfaceHost
    @Environment(\.forgeTheme) private var theme

    init(model: A2uiFeedsModel) {
        self.model = model
        _host = StateObject(wrappedValue: A2uiSurfaceHost(model: model))
    }

    private var reviewing: Binding<Bool> {
        Binding(
            get: { model.changeSet != nil },
            set: { if !$0 { model.cancelReview() } }
        )
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.l) {
                Text("A suggested folder for each of Commonplace's feeds. Commonplace checks the plan again and moves the feeds only when you confirm.")
                    .forgeText(.note)
                if let notice = model.notice {
                    A2uiNoticeView(notice: notice, identifier: "a2uiFeeds.notice")
                }
                if model.started, let surface = host.surface {
                    A2uiSurfaceView(surface: surface, catalog: host.catalog)
                    ForgeButton("Start Over") { await model.start() }
                        .buttonStyle(.forge(.tertiary, expands: true))
                        .disabled(model.busy)
                        .accessibilityIdentifier("a2uiFeeds.startOver")
                } else {
                    ForgeButton("Suggest Folders") { await model.start() }
                        .buttonStyle(.forge(.primary, expands: true))
                        .accessibilityIdentifier("a2uiFeeds.start")
                }
            }
            .padding(theme.spacing.l)
        }
        .background(theme.colors.groupedBackground)
        .navigationTitle("Organise Feeds")
        .inlineNavigationTitle()
        .sheet(isPresented: reviewing) {
            if let changeSet = model.changeSet {
                A2uiFolderReviewSheet(
                    changeSet: changeSet,
                    busy: model.busy,
                    confirm: { Task { await model.confirm() } },
                    cancel: { model.cancelReview() })
            }
        }
    }
}

/// The app-owned confirmation for C3: only Rust's change set.
struct A2uiFolderReviewSheet: View {
    let changeSet: A2uiFolderChangeSet
    let busy: Bool
    let confirm: () -> Void
    let cancel: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section {
                    if changeSet.rows.isEmpty {
                        Text("Nothing would move.").forgeText(.body)
                    }
                    ForEach(changeSet.rows, id: \.feedId) { row in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(row.feedTitle).forgeText(.rowTitle)
                            Text("\(row.from) to \(row.to)").forgeText(.body)
                            if row.newFolder {
                                ForgeBadge("New folder", tone: .info)
                            }
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("a2uiFeeds.change.\(row.feedId)")
                    }
                } header: {
                    Text(changeSet.rows.count == 1 ? "1 feed moves" : "\(changeSet.rows.count) feeds move")
                } footer: {
                    Text("Worked out by the app from your choices. \(changeSet.unchanged) stay where they are. Commonplace checks the plan again before it moves anything.")
                }
            }
            .forgeSwipeGuard(busy)
            .navigationTitle("Review Folders")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: cancel)
                        .disabled(busy)
                        .accessibilityIdentifier("a2uiFeeds.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Confirm", action: confirm)
                        .disabled(changeSet.rows.isEmpty || busy)
                        .accessibilityIdentifier("a2uiFeeds.confirm")
                }
            }
        }
    }
}

/// C4: a proposed playlist from Soundscape's library.
struct A2uiPlaylistView: View {
    let model: A2uiPlaylistModel
    @StateObject private var host: A2uiSurfaceHost
    @Environment(\.forgeTheme) private var theme

    init(model: A2uiPlaylistModel) {
        self.model = model
        _host = StateObject(wrappedValue: A2uiSurfaceHost(model: model))
    }

    private var reviewing: Binding<Bool> {
        Binding(
            get: { model.changeSet != nil },
            set: { if !$0 { model.cancelReview() } }
        )
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.l) {
                Text("A proposed playlist from songs already in Soundscape. Nothing is searched, downloaded or played. Soundscape makes the playlist only when you confirm.")
                    .forgeText(.note)
                if let notice = model.notice {
                    A2uiNoticeView(notice: notice, identifier: "a2uiPlaylist.notice")
                }
                if model.started, let surface = host.surface {
                    A2uiSurfaceView(surface: surface, catalog: host.catalog)
                    ForgeButton("Start Over") { await model.start() }
                        .buttonStyle(.forge(.tertiary, expands: true))
                        .disabled(model.busy)
                        .accessibilityIdentifier("a2uiPlaylist.startOver")
                } else {
                    ForgeButton("Suggest a Playlist") { await model.start() }
                        .buttonStyle(.forge(.primary, expands: true))
                        .accessibilityIdentifier("a2uiPlaylist.start")
                }
            }
            .padding(theme.spacing.l)
        }
        .background(theme.colors.groupedBackground)
        .navigationTitle("Make a Playlist")
        .inlineNavigationTitle()
        .sheet(isPresented: reviewing) {
            if let changeSet = model.changeSet {
                A2uiPlaylistReviewSheet(
                    changeSet: changeSet,
                    busy: model.busy,
                    confirm: { Task { await model.confirm() } },
                    cancel: { model.cancelReview() })
            }
        }
    }
}

/// The app-owned confirmation for C4: only Rust's change set.
struct A2uiPlaylistReviewSheet: View {
    let changeSet: A2uiPlaylistChangeSet
    let busy: Bool
    let confirm: () -> Void
    let cancel: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(Array(changeSet.songs.enumerated()), id: \.element.trackId) { _, song in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(song.title).forgeText(.rowTitle)
                            Text(song.artist).forgeText(.metadata)
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("a2uiPlaylist.song.\(song.trackId)")
                    }
                } header: {
                    Text("\u{201C}\(changeSet.name)\u{201D}, \(songs)")
                } footer: {
                    Text(footer)
                }
            }
            .forgeSwipeGuard(busy)
            .navigationTitle("Review Playlist")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: cancel)
                        .disabled(busy)
                        .accessibilityIdentifier("a2uiPlaylist.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Make Playlist", action: confirm)
                        .disabled(changeSet.songs.isEmpty || busy)
                        .accessibilityIdentifier("a2uiPlaylist.confirm")
                }
            }
        }
    }

    private var songs: String {
        changeSet.songs.count == 1 ? "1 song" : "\(changeSet.songs.count) songs"
    }

    private var footer: String {
        var parts: [String] = []
        let minutes = Int((changeSet.durationSeconds / 60).rounded())
        if minutes > 0 { parts.append("About \(minutes) min.") }
        if changeSet.leftOut > 0 {
            parts.append(changeSet.leftOut == 1 ? "1 song left out." : "\(changeSet.leftOut) songs left out.")
        }
        parts.append("Worked out by the app from your choices. Soundscape checks the songs again before it makes the playlist.")
        return parts.joined(separator: " ")
    }
}

/// C2: the Bonsai Harness's last verified task, read-only.
struct A2uiTaskCardView: View {
    let model: A2uiTaskCardModel
    @StateObject private var host: A2uiSurfaceHost
    @Environment(\.forgeTheme) private var theme

    init(model: A2uiTaskCardModel) {
        self.model = model
        _host = StateObject(wrappedValue: A2uiSurfaceHost(model: model))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.l) {
                Text("A card for the last task the Bonsai Harness verified, built from what the harness saw, never from the model's own summary. It has no controls.")
                    .forgeText(.note)
                if let notice = model.notice {
                    A2uiNoticeView(notice: notice, identifier: "a2uiTaskCard.notice")
                }
                if model.shown, let surface = host.surface {
                    A2uiSurfaceView(surface: surface, catalog: host.catalog)
                }
                Button(model.shown ? "Show Again" : "Show the Card") { model.show() }
                    .buttonStyle(.forge(model.shown ? .tertiary : .primary, expands: true))
                    .disabled(!model.available)
                    .accessibilityIdentifier("a2uiTaskCard.show")
            }
            .padding(theme.spacing.l)
        }
        .background(theme.colors.groupedBackground)
        .navigationTitle("Bonsai Result")
        .inlineNavigationTitle()
    }
}
