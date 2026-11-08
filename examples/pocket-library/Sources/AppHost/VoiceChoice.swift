import Foundation

/// A speech voice, reduced to what choosing one needs. Plain Swift, so the
/// choice compiles and is tested on Linux; `ReadAloud` turns the system's
/// `AVSpeechSynthesisVoice`s into these.
struct VoiceCandidate: Equatable, Identifiable, Sendable {
    enum Quality: Int, Comparable, Sendable {
        /// The small voices every iPhone ships with. They sound robotic.
        case standard = 1
        /// Downloadable, much more natural.
        case enhanced = 2
        /// Downloadable, the most natural (the neural Siri-like voices).
        case premium = 3

        static func < (lhs: Quality, rhs: Quality) -> Bool { lhs.rawValue < rhs.rawValue }
    }

    let id: String
    let name: String
    /// A BCP 47 tag such as "en-GB".
    let language: String
    let quality: Quality
    /// The user's own Personal Voice, offered only once they allow it.
    let isPersonal: Bool
}

/// Which voice reads aloud. iOS's default voice for a language is the
/// small standard one even when a far better one is installed, so the
/// reader asks for one by quality instead.
enum VoiceChoice {
    /// The voices to offer for `language` (the device's, such as "en-GB"),
    /// best first: Personal Voices, then by quality, then voices of the
    /// device's own region, then by name. Falls back to English voices
    /// when none speak the language.
    static func ranked(_ voices: [VoiceCandidate], language: String) -> [VoiceCandidate] {
        var matching = voices.filter { base($0.language) == base(language) }
        if matching.isEmpty { matching = voices.filter { base($0.language) == "en" } }
        let region = normalized(language)
        return matching.sorted { a, b in
            if a.isPersonal != b.isPersonal { return a.isPersonal }
            if a.quality != b.quality { return a.quality > b.quality }
            let aLocal = normalized(a.language) == region
            let bLocal = normalized(b.language) == region
            if aLocal != bLocal { return aLocal }
            return a.name.localizedStandardCompare(b.name) == .orderedAscending
        }
    }

    /// The voice used when the user hasn't picked one: the best-quality
    /// installed voice for the language. A Personal Voice is never chosen
    /// for the user; they pick it themselves.
    static func best(_ voices: [VoiceCandidate], language: String) -> VoiceCandidate? {
        ranked(voices, language: language).first { !$0.isPersonal }
    }

    /// True when only the standard voices are installed, so the user should
    /// be told where to download a better one.
    static func onlyStandard(_ voices: [VoiceCandidate], language: String) -> Bool {
        !ranked(voices, language: language).contains { $0.quality > .standard && !$0.isPersonal }
    }

    /// "Daniel (Enhanced, English (United Kingdom))".
    static func label(for voice: VoiceCandidate, locale: Locale = .current) -> String {
        let region = locale.localizedString(forIdentifier: voice.language) ?? voice.language
        let kind: String? = voice.isPersonal ? "Personal Voice"
            : voice.quality == .premium ? "Premium"
            : voice.quality == .enhanced ? "Enhanced"
            : nil
        return kind.map { "\(voice.name) (\($0), \(region))" } ?? "\(voice.name) (\(region))"
    }

    /// Where the Settings app keeps downloadable voices.
    static let downloadHint = "Settings > Accessibility > Spoken Content > Voices"

    private static func base(_ tag: String) -> String {
        String(normalized(tag).prefix { $0 != "-" })
    }

    private static func normalized(_ tag: String) -> String {
        tag.replacingOccurrences(of: "_", with: "-").lowercased()
    }
}
