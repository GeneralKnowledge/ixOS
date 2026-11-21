// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeNowPlayingRules.swift.

import Foundation

/// What the system did to the app's audio, as `ForgeNowPlaying` sees it.
public enum ForgeAudioEvent: Equatable, Sendable {
    /// A call, Siri, an alarm or another app's audio took the sound.
    case interruptionBegan
    /// The interruption is over. `shouldResume` is iOS saying the app may
    /// carry on: a call or Siri hands the sound back, another app that
    /// started its own music doesn't.
    case interruptionEnded(shouldResume: Bool)
    /// The output in use went away: headphones unplugged, AirPods taken out
    /// or out of range.
    case outputGone
}

/// What a player should do about an `ForgeAudioEvent`.
public enum ForgeAudioReaction: Equatable, Sendable {
    case pause
    case play
    case nothing
}

/// The rules a player follows for the system's audio events, the ones
/// Soundscape's player passed on the iPhone with (#306): pause when
/// interrupted, carry on afterwards only if it was playing and iOS says it
/// may, and pause when headphones go. A plain value, so the rules are
/// tested on Linux; `ForgeNowPlaying` feeds it the real notifications.
public struct ForgeAudioRules: Equatable, Sendable {
    /// Playing when the current interruption began.
    public private(set) var interruptedWhilePlaying = false

    public init() {}

    public mutating func react(to event: ForgeAudioEvent, playing: Bool) -> ForgeAudioReaction {
        switch event {
        case .interruptionBegan:
            interruptedWhilePlaying = playing
            return playing ? .pause : .nothing
        case .interruptionEnded(let shouldResume):
            let carryOn = interruptedWhilePlaying && shouldResume
            interruptedWhilePlaying = false
            return carryOn ? .play : .nothing
        case .outputGone:
            // Never carry on out loud from the speaker.
            return playing ? .pause : .nothing
        }
    }
}
