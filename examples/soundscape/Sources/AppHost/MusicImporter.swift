#if os(iOS)
import AVFoundation
@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import Foundation

private let log = ForgeLog("Import")

/// Reads music files the person picked in Files: each file's tags (title,
/// artist, album, track number, genre, length, cover) with AVFoundation, for
/// Rust to keep with its copy (`import_music`). Rust copies the files; this
/// only reads them.
enum MusicImporter {
    /// Imports `urls` (security-scoped, from the Files picker) into the
    /// library, holding access to them until Rust has copied them.
    @MainActor
    static func importFiles(_ urls: [URL], into model: ListenModel) async -> ImportResult? {
        let scoped = urls.filter { $0.startAccessingSecurityScopedResource() }
        defer { scoped.forEach { $0.stopAccessingSecurityScopedResource() } }
        let covers = FileManager.default.temporaryDirectory
            .appendingPathComponent("import-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: covers, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: covers) }
        var files: [MusicFile] = []
        for (index, url) in urls.enumerated() {
            files.append(await read(url, covers: covers, index: index))
        }
        return await model.importMusic(files)
    }

    /// One file's tags. Whatever can't be read is left out; Rust names a
    /// file without a title after the file.
    static func read(_ url: URL, covers: URL, index: Int) async -> MusicFile {
        let asset = AVURLAsset(url: url)
        var title: String?
        var artist: String?
        var album: String?
        var genre: String?
        var number: UInt32?
        var cover: String?
        var duration: Double?
        if let length = try? await asset.load(.duration), length.isNumeric, length.seconds > 0 {
            duration = length.seconds
        }
        let common = (try? await asset.load(.commonMetadata)) ?? []
        for item in common {
            switch item.commonKey {
            case .commonKeyTitle?: title = title ?? (try? await item.load(.stringValue))
            case .commonKeyArtist?: artist = artist ?? (try? await item.load(.stringValue))
            case .commonKeyAlbumName?: album = album ?? (try? await item.load(.stringValue))
            case .commonKeyArtwork?:
                if cover == nil, let data = try? await item.load(.dataValue) {
                    cover = write(data, to: covers, index: index)
                }
            default: break
            }
        }
        // Track number and genre aren't common keys: iTunes (M4A) and ID3 (MP3) say them their own way.
        let all = (try? await asset.load(.metadata)) ?? []
        for item in all {
            switch item.identifier {
            case .iTunesMetadataTrackNumber?:
                // Big-endian: two bytes of padding, then the number.
                if number == nil, let data = try? await item.load(.dataValue), data.count >= 4 {
                    let bytes = [UInt8](data)
                    number = UInt32(bytes[2]) << 8 | UInt32(bytes[3])
                }
            case .id3MetadataTrackNumber?:
                // "3/12"
                if number == nil, let text = try? await item.load(.stringValue) {
                    number = text.split(separator: "/").first.flatMap { UInt32($0.trimmingCharacters(in: .whitespaces)) }
                }
            case .iTunesMetadataUserGenre?, .id3MetadataContentType?, .quickTimeMetadataGenre?:
                if genre == nil { genre = try? await item.load(.stringValue) }
            default: break
            }
        }
        log.info("read tags of file \(index)")
        return MusicFile(
            path: url.path, fileName: url.lastPathComponent, title: title, artist: artist, album: album,
            number: number, genre: genre, durationSeconds: duration, artworkPath: cover
        )
    }

    /// Writes a cover picture as the file type its bytes say (PNG or JPEG).
    private static func write(_ data: Data, to folder: URL, index: Int) -> String? {
        let png = data.starts(with: [0x89, 0x50, 0x4E, 0x47])
        let url = folder.appendingPathComponent("cover-\(index).\(png ? "png" : "jpg")")
        do {
            try data.write(to: url)
            return url.path
        } catch {
            return nil
        }
    }
}
#endif
