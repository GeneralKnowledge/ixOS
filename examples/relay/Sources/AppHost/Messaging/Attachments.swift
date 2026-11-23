@preconcurrency import RelayFFI
import ForgeAppKit
import ForgeUI
import Foundation
import ImageIO
import SwiftUI
import UniformTypeIdentifiers

/// Photos and documents (M3). Rust keeps, uploads, downloads and checks the
/// files; Swift picks them, shrinks photos, draws thumbnails and previews.

/// Attachment files under their own names, for Quick Look, thumbnails and
/// the share sheet. Here rather than in ChatModel because naming a file
/// after its type needs UniformTypeIdentifiers, which Linux lacks.
extension ChatModel {
    /// The attachment under its own name if it is already on this iPhone
    /// (sent from here, or opened before), without fetching it. For
    /// thumbnails of documents, which aren't downloaded until opened.
    public func storedFile(for attachment: ChatAttachment) -> URL? {
        guard !attachment.isFolder, let path = core?.attachmentFile(attachmentId: attachment.id) else { return nil }
        return try? AttachmentFiles.namedCopy(of: URL(fileURLWithPath: path), for: attachment)
    }

    /// A folder a trusted device sent (or this iPhone sent it), in the
    /// Files app: Quick Look can't show a folder. Nil if it is gone.
    public func folderURL(for attachment: ChatAttachment) -> URL? {
        guard attachment.isFolder, let path = core?.attachmentFile(attachmentId: attachment.id) else {
            return nil
        }
        let file = URL(fileURLWithPath: path, isDirectory: true).absoluteString
        return URL(string: file.replacingOccurrences(of: "file://", with: "shareddocuments://"))
    }

    /// A file for Quick Look and the share sheet: the attachment under its
    /// own name. Shows an alert if it cannot be fetched.
    public func previewFile(for attachment: ChatAttachment) async -> URL? {
        guard let core else { return nil }
        do {
            let path: String
            if let local = core.attachmentFile(attachmentId: attachment.id) {
                path = local
            } else {
                path = try await core.fetchAttachment(attachmentId: attachment.id)
            }
            return try AttachmentFiles.namedCopy(
                of: URL(fileURLWithPath: path), for: attachment)
        } catch {
            actionError = Self.failure(error)
            return nil
        }
    }
}

/// A file picked in the composer, waiting to be sent with the next message.
/// `url` is a private copy in the temporary directory, in a folder of its
/// own: Rust copies it again into its attachment store when it is sent, so
/// the composer calls `release()` once the send has finished or the file is
/// removed from the message.
struct StagedFile: Identifiable, Equatable {
    let id = UUID()
    let url: URL
    let name: String
    let mime: String
    /// The picked document's copy, which owns `url`; nil for a photo, whose
    /// JPEG Chat wrote itself.
    var picked: ForgePickedFile? = nil

    /// Deletes the private copy. Safe to call more than once.
    func release() {
        if let picked {
            picked.release()
        } else {
            try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
        }
    }

    var outgoing: ChatOutgoingFile {
        ChatOutgoingFile(path: url.path, name: name, mime: mime)
    }

    var isImage: Bool { mime.hasPrefix("image/") }
}

enum AttachmentFiles {
    /// Photos are sent at most this many pixels on their longer side.
    // swift-only: Swift scales the photo before Rust sees it.
    static let maxPhotoPixels = 2048

    /// A fresh folder in the temporary directory for one picked file.
    private static func stagingFolder() throws -> URL {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("ChatStaging", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// A picked photo as a JPEG of at most `maxPhotoPixels`, the right way
    /// up, without its location or other metadata (`ForgeImages`). Runs off
    /// the main actor.
    static func preparePhoto(_ data: Data, takenAt date: Date = .now) async throws -> StagedFile {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyyMMdd_HHmmss"
        let name = "IMG_\(formatter.string(from: date)).jpg"
        let url = try stagingFolder().appendingPathComponent(name)
        try await ForgeImages.writeDownscaledJPEG(from: data, maxPixels: maxPhotoPixels, to: url)
        return StagedFile(url: url, name: name, mime: "image/jpeg")
    }

    /// A document from the Files picker, copied off the main thread while
    /// its security scope is open (ForgeAppKit's `ForgePickedFile`; the
    /// picker's URL may stop working afterwards, and a large document takes
    /// seconds to copy).
    static func prepareDocument(_ picked: URL) async throws -> StagedFile {
        let copy = try await ForgePickedFile.copy(of: picked)
        let type = UTType(filenameExtension: picked.pathExtension)
        return StagedFile(
            url: copy.url, name: copy.name,
            mime: type?.preferredMIMEType ?? "application/octet-stream", picked: copy)
    }

    /// Rust names its files by their id, without an extension, so Quick Look
    /// and the share sheet get a copy (a hard link where possible) under the
    /// attachment's own name.
    static func namedCopy(of file: URL, for attachment: ChatAttachment) throws -> URL {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("ChatPreview", isDirectory: true)
            .appendingPathComponent(attachment.id, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let named = folder.appendingPathComponent(displayFileName(attachment))
        if FileManager.default.fileExists(atPath: named.path) { return named }
        do {
            try FileManager.default.linkItem(at: file, to: named)
        } catch {
            try FileManager.default.copyItem(at: file, to: named)
        }
        return named
    }

    /// The attachment's name as a safe file name, with an extension that
    /// matches its type.
    static func displayFileName(_ attachment: ChatAttachment) -> String {
        var name = attachment.name
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-")
        if name.isEmpty || name.hasPrefix(".") { name = "file" + name }
        if (name as NSString).pathExtension.isEmpty,
            let ext = UTType(mimeType: attachment.mime)?.preferredFilenameExtension
        {
            name += ".\(ext)"
        }
        return name
    }

    /// A thumbnail of the image at `url`, at most `maxPixels` on its longer
    /// side, decoded off the main actor (`ForgeImages`).
    static func thumbnail(of url: URL, maxPixels: Int) async -> CGImage? {
        await ForgeImages.thumbnail(of: url, maxPixels: maxPixels)
    }

    /// "0:07", "1:23".
    static func timeLabel(_ seconds: TimeInterval) -> String {
        Duration.seconds(max(0, seconds.rounded())).formatted(.time(pattern: .minuteSecond))
    }

    /// The name a voice message is sent under.
    static let voiceMessageName = "Voice message.m4a"

    /// "2 KB", "1.4 MB".
    static func sizeLabel(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }

    /// An SF Symbol for a document's type.
    static func symbol(for mime: String) -> String {
        if mime == ChatAttachment.folderMime { return "folder" }
        if mime == "application/pdf" { return "doc.richtext" }
        if mime.hasPrefix("text/") { return "doc.text" }
        if mime.hasPrefix("audio/") { return "waveform" }
        if mime.hasPrefix("video/") { return "film" }
        if mime.hasPrefix("image/") { return "photo" }
        if mime.contains("zip") { return "doc.zipper" }
        return "doc"
    }
}

extension ChatAttachment {
    /// A voice message (or any audio file): played inline in its bubble.
    var isAudio: Bool { mime.hasPrefix("audio/") }

    /// The type of a folder sent to or from a trusted device as one
    /// attachment (rust/src/device_chat.rs, `FOLDER_MIME`).
    static let folderMime = "inode/directory"

    /// A folder: it opens in the Files app.
    var isFolder: Bool { mime == Self.folderMime }
}

extension ChatMessage {
    /// The text to show where only a line fits (a list row, a quote): the
    /// body, or "Photo" / the file name for a message without text.
    var summary: String {
        if !body.isEmpty || attachments.isEmpty { return body }
        let first = attachments[0]
        let what =
            first.isImage
            ? String(localized: "Photo")
            : first.isAudio ? String(localized: "Voice message") : first.name
        return attachments.count > 1 ? "\(what) +\(attachments.count - 1)" : what
    }
}

/// The attachments at the top of a bubble: photos as thumbnails, voice
/// messages as players, other files as rows. Tapping a photo or file opens
/// it in Quick Look.
struct AttachmentsView: View {
    let model: ChatModel
    let attachments: [ChatAttachment]
    let onMine: Bool
    let open: (ChatAttachment) -> Void
    let play: (ChatAttachment) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(attachments.enumerated()), id: \.offset) { _, attachment in
                if attachment.isAudio {
                    VoiceMessageRow(
                        model: model, attachment: attachment, onMine: onMine,
                        play: { play(attachment) })
                } else {
                    Button {
                        open(attachment)
                    } label: {
                        if attachment.isImage {
                            PhotoThumbnail(model: model, attachment: attachment)
                        } else {
                            DocumentRow(model: model, attachment: attachment, onMine: onMine)
                        }
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(
                        attachment.isImage
                            ? String(localized: "Photo, \(attachment.name)")
                            : String(
                                localized:
                                    "Document, \(attachment.name), \(AttachmentFiles.sizeLabel(attachment.size))"
                            )
                    )
                    .accessibilityHint("Opens it")
                }
            }
        }
    }
}

/// A photo in a bubble. The file is fetched (once) and thumbnailed off the
/// main actor when the bubble appears.
private struct PhotoThumbnail: View {
    let model: ChatModel
    let attachment: ChatAttachment
    @State private var image: CGImage?
    @State private var failed = false

    var body: some View {
        ForgeThumbnail(width: 220) {
            if let image {
                Image(decorative: image, scale: 1)
                    .resizable()
                    .scaledToFill()
            } else {
                ForgeThumbnailPlaceholder(failed ? .failed : .loading, systemImage: "photo")
            }
        }
        .task(id: attachment.id) {
            guard image == nil else { return }
            guard let url = await model.localFile(for: attachment) else {
                failed = true
                return
            }
            // Twice the point size, for Retina screens.
            image = await AttachmentFiles.thumbnail(of: url, maxPixels: 440)
            failed = image == nil
        }
    }
}

/// A document in a bubble: a picture of its first page when it is on this
/// iPhone (Quick Look's thumbnail, `ForgeDocuments`), else its type's
/// symbol; then its name and size. Documents are fetched only when opened.
private struct DocumentRow: View {
    let model: ChatModel
    let attachment: ChatAttachment
    let onMine: Bool
    @Environment(\.displayScale) private var displayScale
    @State private var page: CGImage?

    var body: some View {
        HStack(spacing: 10) {
            preview
            VStack(alignment: .leading, spacing: 2) {
                Text(attachment.name)
                    .font(.callout.weight(.medium))
                    .lineLimit(2)
                Text(AttachmentFiles.sizeLabel(attachment.size))
                    .font(.caption)
                    .opacity(0.8)
            }
        }
        .padding(8)
        .frame(minWidth: 180, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(onMine ? Color.white.opacity(0.2) : Color.primary.opacity(0.06))
        )
        .task(id: attachment.id) {
            guard page == nil, let url = model.storedFile(for: attachment) else { return }
            page = await ForgeDocuments.thumbnail(
                of: url, size: CGSize(width: 32, height: 42), scale: displayScale)
        }
    }

    @ViewBuilder
    private var preview: some View {
        if let page {
            ForgeThumbnail(width: 32, aspectRatio: 32 / 42) {
                Image(decorative: page, scale: displayScale)
                    .resizable()
                    .scaledToFit()
            }
        } else {
            Image(systemName: AttachmentFiles.symbol(for: attachment.mime))
                .font(.title2)
                .frame(width: 32)
        }
    }
}

/// A voice message in a bubble: play/pause, its waveform (filled as it
/// plays) and its length. The file is fetched and measured when the bubble
/// appears; voice messages are small.
private struct VoiceMessageRow: View {
    let model: ChatModel
    let attachment: ChatAttachment
    let onMine: Bool
    let play: () -> Void
    @State private var url: URL?
    @State private var bars: [Float] = []
    @State private var duration: TimeInterval?
    @State private var failed = false

    private var player: ForgeAudioPlayer { ForgeAudioPlayer.shared }
    private var playing: Bool { url != nil && player.current == url && player.isPlaying }
    private var progress: Double { url.map { player.progress(of: $0) } ?? 0 }

    var body: some View {
        HStack(spacing: 10) {
            Button(playing ? "Pause" : "Play", systemImage: playing ? "pause.fill" : "play.fill", action: play)
                .labelStyle(.iconOnly)
                .font(.title2)
                .frame(width: 32, height: 32)
                .disabled(url == nil)
            if failed {
                Label("Not available", systemImage: "exclamationmark.triangle")
                    .font(.caption)
            } else {
                WaveformView(bars: bars, progress: progress, onMine: onMine)
                    .frame(width: 140, height: 28)
            }
            Text(AttachmentFiles.timeLabel(playing || progress > 0 ? player.position : duration ?? 0))
                .font(.caption.monospacedDigit())
                .opacity(0.8)
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            String(localized: "Voice message, \(AttachmentFiles.timeLabel(duration ?? 0))"))
        .accessibilityAddTraits(.isButton)
        .accessibilityAction { play() }
        .task(id: attachment.id) {
            guard url == nil else { return }
            guard let file = await model.localFile(for: attachment) else {
                failed = true
                return
            }
            duration = try? ForgeAudio.duration(of: file)
            bars = (try? await ForgeAudio.waveform(of: file, bars: 28)) ?? []
            url = file
        }
    }
}

/// Bars for a voice message, filled up to `progress` (0 to 1).
struct WaveformView: View {
    let bars: [Float]
    let progress: Double
    let onMine: Bool

    var body: some View {
        GeometryReader { geometry in
            let count = max(bars.count, 1)
            let step = geometry.size.width / CGFloat(count)
            HStack(alignment: .center, spacing: step * 0.35) {
                ForEach(Array(bars.enumerated()), id: \.offset) { index, level in
                    Capsule()
                        .fill(Double(index) / Double(count) < progress ? played : unplayed)
                        .frame(height: max(3, geometry.size.height * CGFloat(level)))
                }
            }
            .frame(maxHeight: .infinity)
        }
        .accessibilityHidden(true)
    }

    private var played: Color { onMine ? .white : .accentColor }
    private var unplayed: Color { onMine ? .white.opacity(0.4) : .secondary.opacity(0.4) }
}

/// Files picked for the next message, above the text field. Tap × to drop
/// one.
struct StagedFilesBar: View {
    let files: [StagedFile]
    let remove: (StagedFile) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(files) { file in
                    HStack(spacing: 6) {
                        Image(systemName: file.isImage ? "photo" : AttachmentFiles.symbol(for: file.mime))
                        Text(file.name).lineLimit(1)
                        Button("Remove", systemImage: "xmark.circle.fill") { remove(file) }
                            .labelStyle(.iconOnly)
                            .foregroundStyle(.secondary)
                    }
                    .font(.caption)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(Capsule().fill(Color.secondary.opacity(0.15)))
                }
            }
        }
    }
}
