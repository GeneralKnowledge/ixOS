@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import Foundation
import Observation

private let log = ForgeLog("LiveRadio")

/// Live radio's data: stations from the Radio Browser directory, and the
/// stations the person saved or played lately. The rules are Rust's
/// (`rust/src/live_radio.rs`); playback is `LivePlayer`.
/// Foundation and Observation only, so it compiles and tests on Linux.
@Observable
@MainActor
public final class LiveRadioModel {
    public let core: SoundscapeCore
    /// Saved stations (Library > Stations) and recently played ones.
    public private(set) var kept: LiveRadio
    /// Why saving or removing a station failed, for an alert.
    public var problem: String?

    public init(core: SoundscapeCore) {
        self.core = core
        kept = core.liveRadio()
    }

    public func isSaved(_ uuid: String) -> Bool {
        kept.saved.contains { $0.uuid == uuid }
    }

    /// Stations for `ask`, most played first. Cancelled with the screen.
    public func stations(_ ask: LiveAsk) async -> Result<[LiveStation], ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.liveStations(ask: ask, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !ListenModel.isStop(error) { log.warn("stations failed: \(error)") }
            return .failure(ListenFailure(message: ListenModel.message(error), stopped: ListenModel.isStop(error)))
        }
    }

    /// Countries, languages or genres, those with most stations first.
    public func categories(_ kind: LiveCategoryKind) async -> Result<[LiveCategory], ListenFailure> {
        do {
            let found = try await withRustCancellation { cancellation in
                try await core.liveCategories(kind: kind, cancellation: cancellation)
            }
            return .success(found)
        } catch {
            if !ListenModel.isStop(error) { log.warn("categories failed: \(error)") }
            return .failure(ListenFailure(message: ListenModel.message(error), stopped: ListenModel.isStop(error)))
        }
    }

    /// The station to play: looked up again when its stream address may
    /// be old. Throws `NotInLibrary` when Radio Browser no longer lists it.
    public func tune(_ station: LiveStation) async throws -> LiveStation {
        let tuned = try await core.tuneLiveStation(station: station, cancellation: nil)
        kept = core.liveRadio()
        return tuned
    }

    /// After a stream failed: the station's current details, or nil when
    /// Radio Browser no longer lists it.
    public func failed(_ uuid: String) async throws -> LiveStation? {
        let found = try await core.liveStationFailed(uuid: uuid, cancellation: nil)
        kept = core.liveRadio()
        return found
    }

    /// Saves the station to Library > Stations, or removes it.
    public func toggleSaved(_ station: LiveStation) async {
        do {
            if isSaved(station.uuid) {
                _ = try await core.unsaveLiveStation(uuid: station.uuid)
            } else {
                _ = try await core.saveLiveStation(station: station)
            }
            // Read now, not the call's own answer: a slower earlier call
            // can't put back an older list.
            kept = core.liveRadio()
        } catch {
            log.warn("save station failed: \(error)")
            problem = ListenModel.message(error)
        }
    }

    public func clearRecent() async {
        do {
            _ = try await core.clearRecentLiveStations()
            kept = core.liveRadio()
        } catch {
            problem = ListenModel.message(error)
        }
    }
}

extension LiveStation: Identifiable {
    public var id: String { uuid }
}

extension LiveCategory: Identifiable {
    public var id: String { "\(kind):\(key)" }
}

extension LiveStation {
    /// "Japan · Jazz · AAC+ 64 kbps": where it is from, what it plays, and
    /// its stream.
    public var summary: String {
        var parts: [String] = []
        if !country.isEmpty { parts.append(country) }
        if let tag = tags.first { parts.append(tag.capitalized) }
        parts.append(streamDescription)
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// "AAC+ 64 kbps", "HLS" or "".
    public var streamDescription: String {
        if hls { return "HLS" }
        let rate = bitrateKbps > 0 ? "\(bitrateKbps) kbps" : ""
        return [codec, rate].filter { !$0.isEmpty }.joined(separator: " ")
    }
}
