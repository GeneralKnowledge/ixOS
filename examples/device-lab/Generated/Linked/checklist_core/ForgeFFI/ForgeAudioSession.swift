// Copied into every app's Generated/ForgeFFI by scripts/generate-bindings.sh.
// Do not edit the copy; the source of truth is
// crates/forge-services/swift/helpers/ForgeAudioSession.swift.

#if os(iOS)
    import AVFoundation
#endif

/// The app's audio session: what kind of audio it is making now. On iOS
/// this sets `AVAudioSession`; elsewhere it does nothing.
///
/// Setting the session needs no permission, so this is in every app: an app
/// that only plays (Pocket Library's read-aloud) uses it without selecting
/// the `audio` service and its microphone permission. Playing on with the
/// screen locked also needs `background_modes = ["audio"]` in forge.toml.
public enum ForgeAudioSession {
    public enum Use {
        /// Playing speech, even with the ring/silent switch on silent, and
        /// pausing other apps' audio.
        case playback
        /// Recording from the microphone, playing through the speaker.
        case record
    }

    public static func activate(_ use: Use) throws {
        #if os(iOS)
            let session = AVAudioSession.sharedInstance()
            switch use {
            case .playback:
                try session.setCategory(.playback, mode: .spokenAudio)
            case .record:
                try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
            }
            try session.setActive(true)
        #endif
    }

    /// Lets other apps' audio resume.
    public static func deactivate() {
        #if os(iOS)
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        #endif
    }
}
