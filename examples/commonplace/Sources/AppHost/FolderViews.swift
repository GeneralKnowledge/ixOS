@preconcurrency import CommonplaceFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// What a feed or folder row asks the screen to do: the dialogs and sheets
/// are the screen's (`libraryActions`), so the feed list and a folder's
/// screen behave the same.
enum LibraryAction: Identifiable {
    case renameFeed(Feed)
    case unsubscribe(Feed)
    case moveFeed(Feed)
    case newFolder(in: Int64?)
    case renameFolder(Folder)
    case deleteFolder(Folder)

    var id: String {
        switch self {
        case .renameFeed(let feed): return "renameFeed.\(feed.id)"
        case .unsubscribe(let feed): return "unsubscribe.\(feed.id)"
        case .moveFeed(let feed): return "moveFeed.\(feed.id)"
        case .newFolder(let parent): return "newFolder.\(parent.map { String($0) } ?? "top")"
        case .renameFolder(let folder): return "renameFolder.\(folder.id)"
        case .deleteFolder(let folder): return "deleteFolder.\(folder.id)"
        }
    }
}

/// A screen's folders, then its feeds: rows that open them, with their
/// context menus and swipe actions.
struct LibraryRows: View {
    let model: LibraryModel
    /// The folder whose contents these are (nil: the top level).
    let folderID: Int64?
    @Binding var action: LibraryAction?

    var body: some View {
        ForEach(model.folders(in: folderID)) { folder in
            NavigationLink(value: Route.folder(folder.id)) {
                FolderRow(folder: folder)
            }
            .accessibilityIdentifier("feeds.folder.\(folder.id)")
            .contextMenu {
                Button("Rename", systemImage: "pencil") { action = .renameFolder(folder) }
                Button("Mark All as Read", systemImage: "checkmark.circle") {
                    Task { await model.markAllRead(folderID: folder.id) }
                }
                Button("Delete Folder", systemImage: "folder.badge.minus", role: .destructive) {
                    action = .deleteFolder(folder)
                }
            }
        }
        ForEach(model.feeds(in: folderID)) { feed in
            NavigationLink(value: Route.timeline(.feed(feedId: feed.id))) {
                FeedRow(feed: feed)
            }
            .accessibilityIdentifier("feeds.feed.\(feed.id)")
            .swipeActions {
                Button("Unsubscribe", systemImage: "trash", role: .destructive) {
                    action = .unsubscribe(feed)
                }
                .accessibilityIdentifier("feeds.unsubscribe")
            }
            .contextMenu {
                Button("Rename", systemImage: "pencil") { action = .renameFeed(feed) }
                Button("Move to Folder", systemImage: "folder") { action = .moveFeed(feed) }
                Button(
                    feed.notify ? "Don't Notify About New Articles" : "Notify About New Articles",
                    systemImage: feed.notify ? "bell.slash" : "bell"
                ) {
                    Task { await model.setNotify(feed, !feed.notify) }
                }
                Button("Mark All as Read", systemImage: "checkmark.circle") {
                    Task { await model.markAllRead(feedID: feed.id) }
                }
                Button("Unsubscribe", systemImage: "trash", role: .destructive) {
                    action = .unsubscribe(feed)
                }
            }
        }
    }
}

struct FolderRow: View {
    let folder: Folder

    var body: some View {
        ForgeRow(
            folder.name,
            subtitle: inflected("^[\(Int(folder.feedCount)) feed](inflect: true)"),
            systemImage: "folder.fill", tone: .accent
        ) {
            if folder.unreadCount > 0 {
                Text(folder.unreadCount, format: .number)
                    .accessibilityLabel("\(folder.unreadCount) unread")
            }
        }
    }
}

struct FeedRow: View {
    let feed: Feed

    var body: some View {
        ForgeRow(
            feed.title,
            subtitle: feed.lastError.map(LibraryModel.refreshProblem),
            systemImage: feed.lastError == nil ? "dot.radiowaves.up.forward" : "exclamationmark.triangle.fill",
            tone: feed.lastError == nil ? .accent : .danger
        ) {
            if feed.unreadCount > 0 {
                Text(feed.unreadCount, format: .number)
                    .accessibilityLabel("\(feed.unreadCount) unread")
            }
        }
    }
}

/// A folder's screen: everything in it as one timeline, then its folders
/// and feeds.
struct FolderView: View {
    let model: LibraryModel
    let folderID: Int64
    @State private var action: LibraryAction?

    var body: some View {
        List {
            if let folder = model.folder(id: folderID) {
                Section {
                    NavigationLink(value: Route.timeline(.folder(folderId: folderID))) {
                        ForgeRow("All in \(folder.name)", systemImage: "tray.full.fill", tone: .neutral) {
                            if folder.unreadCount > 0 {
                                Text(folder.unreadCount, format: .number)
                                    .accessibilityLabel("\(folder.unreadCount) unread")
                            }
                        }
                    }
                    .accessibilityIdentifier("folder.all")
                }
                Section {
                    LibraryRows(model: model, folderID: folderID, action: $action)
                }
            }
        }
        .overlay {
            if model.folder(id: folderID) == nil {
                ForgeStateView.empty("Folder Deleted", systemImage: "folder",
                                     description: "Its feeds moved up a level.")
            } else if model.folders(in: folderID).isEmpty && model.feeds(in: folderID).isEmpty {
                ForgeStateView.empty("Empty Folder", systemImage: "folder",
                                     description: "Move a feed here from its menu: touch and hold it in the feed list.")
            }
        }
        .navigationTitle(model.folder(id: folderID)?.name ?? "Folder")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu("Folder", systemImage: "ellipsis.circle") {
                    Button("New Folder Inside", systemImage: "folder.badge.plus") {
                        action = .newFolder(in: folderID)
                    }
                    .accessibilityIdentifier("folder.newFolder")
                    if let folder = model.folder(id: folderID) {
                        Button("Rename", systemImage: "pencil") { action = .renameFolder(folder) }
                            .accessibilityIdentifier("folder.rename")
                        Button("Delete Folder", systemImage: "folder.badge.minus", role: .destructive) {
                            action = .deleteFolder(folder)
                        }
                        .accessibilityIdentifier("folder.delete")
                    }
                }
                .accessibilityIdentifier("folder.menu")
            }
        }
        .libraryActions($action, model: model)
    }
}

extension View {
    /// The dialogs and sheets `LibraryAction`s open.
    func libraryActions(_ action: Binding<LibraryAction?>, model: LibraryModel) -> some View {
        modifier(LibraryActions(action: action, model: model))
    }
}

private struct LibraryActions: ViewModifier {
    @Binding var action: LibraryAction?
    let model: LibraryModel
    @State private var name = ""

    func body(content: Content) -> some View {
        content
            .alert(nameTitle, isPresented: isNaming) {
                TextField("Name", text: $name)
                    .accessibilityIdentifier("library.name")
                Button("Cancel", role: .cancel) {}
                    .accessibilityIdentifier("library.name.cancel")
                Button(nameButton) { saveName() }
                    .accessibilityIdentifier("library.name.save")
            } message: {
                if case .renameFeed = action {
                    Text("Leave it empty to use the feed's own name.")
                }
            }
            .confirmationDialog(confirmTitle, isPresented: isConfirming, titleVisibility: .visible) {
                switch action {
                case .unsubscribe(let feed):
                    Button("Unsubscribe", role: .destructive) {
                        Task { await model.remove(feed) }
                    }
                    .accessibilityIdentifier("feeds.unsubscribe.confirm")
                case .deleteFolder(let folder):
                    Button("Delete Folder", role: .destructive) {
                        Task { await model.delete(folder) }
                    }
                    .accessibilityIdentifier("folder.delete.confirm")
                default:
                    EmptyView()
                }
            } message: {
                switch action {
                case .unsubscribe:
                    Text("Its articles are removed from this device, bookmarked ones too.")
                case .deleteFolder:
                    Text("Its feeds and folders move up a level. No subscription is removed.")
                default:
                    EmptyView()
                }
            }
            .sheet(item: movingFeed) { feed in
                MoveFeedView(model: model, feed: feed)
            }
            .onChange(of: action?.id) {
                switch action {
                case .renameFeed(let feed): name = feed.title
                case .renameFolder(let folder): name = folder.name
                default: name = ""
                }
            }
    }

    private var isNaming: Binding<Bool> {
        Binding(
            get: {
                switch action {
                case .renameFeed, .newFolder, .renameFolder: return true
                default: return false
                }
            },
            set: { if !$0 { action = nil } })
    }

    private var isConfirming: Binding<Bool> {
        Binding(
            get: {
                switch action {
                case .unsubscribe, .deleteFolder: return true
                default: return false
                }
            },
            set: { if !$0 { action = nil } })
    }

    private var movingFeed: Binding<Feed?> {
        Binding(
            get: { if case .moveFeed(let feed) = action { return feed } else { return nil } },
            set: { if $0 == nil { action = nil } })
    }

    private var nameTitle: String {
        switch action {
        case .renameFeed: return String(localized: "Rename Feed")
        case .newFolder: return String(localized: "New Folder")
        case .renameFolder: return String(localized: "Rename Folder")
        default: return ""
        }
    }

    private var nameButton: String {
        if case .newFolder = action { return String(localized: "Create") }
        return String(localized: "Rename")
    }

    private var confirmTitle: String {
        switch action {
        case .unsubscribe(let feed): return String(localized: "Unsubscribe from \(feed.title)?")
        case .deleteFolder(let folder): return String(localized: "Delete the folder \(folder.name)?")
        default: return ""
        }
    }

    private func saveName() {
        let typed = name
        switch action {
        case .renameFeed(let feed): Task { await model.rename(feed, to: typed) }
        case .newFolder(let parent): Task { await model.createFolder(named: typed, in: parent) }
        case .renameFolder(let folder): Task { await model.rename(folder, to: typed) }
        default: break
        }
    }
}

/// Move to Folder: the top level, every folder (indented by depth), or a
/// new one.
private struct MoveFeedView: View {
    let model: LibraryModel
    let feed: Feed
    @State private var newFolder = ""
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    choice("No Folder", systemImage: "tray", folderID: nil, depth: 0)
                    ForEach(model.folders) { folder in
                        choice(folder.name, systemImage: "folder", folderID: folder.id, depth: Int(folder.depth))
                    }
                }
                Section("New Folder") {
                    HStack {
                        TextField("Name", text: $newFolder)
                            .accessibilityIdentifier("moveFeed.newFolder")
                            .onSubmit(createAndMove)
                        Button("Create and Move", action: createAndMove)
                            .disabled(newFolder.trimmingCharacters(in: .whitespaces).isEmpty)
                            .accessibilityIdentifier("moveFeed.create")
                    }
                }
            }
            .navigationTitle("Move \(feed.title)")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("moveFeed.cancel")
                }
            }
        }
    }

    private func choice(_ name: String, systemImage: String, folderID: Int64?, depth: Int) -> some View {
        Button {
            Task {
                await model.move(feed, to: folderID)
                dismiss()
            }
        } label: {
            HStack {
                Label(name, systemImage: systemImage)
                    .padding(.leading, CGFloat(depth) * 16)
                Spacer()
                if model.feed(id: feed.id)?.folderId == folderID {
                    Image(systemName: "checkmark")
                        .foregroundStyle(.tint)
                        .accessibilityLabel("Current folder")
                }
            }
        }
        .accessibilityIdentifier("moveFeed.folder.\(folderID.map { String($0) } ?? "none")")
    }

    private func createAndMove() {
        let name = newFolder
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        Task {
            await model.move(feed, toNewFolder: name)
            dismiss()
        }
    }
}

/// A string for the screen, with Foundation's grammar agreement applied ("^[3 feed](inflect:
/// true)" is "3 feeds"); `String(localized:)` alone leaves the markup in.
func inflected(_ key: String.LocalizationValue) -> String {
    String(AttributedString(localized: key).characters)
}
