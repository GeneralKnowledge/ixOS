@preconcurrency import SoundscapeFFI
import Foundation

/// A smart playlist as its editor changes it: plain values the form binds
/// to, turned into Rust's `SmartRules` to count and save. Foundation only,
/// so it tests on Linux; the rules themselves are checked in Rust
/// (`rust/src/smart.rs`).
public struct SmartPlaylistDraft: Equatable {
    public var name: String
    public var matchAll: Bool
    public var rules: [SmartRuleDraft]
    public var sort: SmartSort
    public var limited: Bool
    public var limit: Int

    public init(name: String, rules: SmartRules) {
        self.name = name
        matchAll = rules.matchAll
        self.rules = rules.rules.map(SmartRuleDraft.init)
        sort = rules.sort
        limited = rules.limit != nil
        limit = Int(rules.limit ?? 25)
    }

    /// A new smart playlist starts with one rule, so it isn't every song.
    public static var new: SmartPlaylistDraft {
        var draft = SmartPlaylistDraft(
            name: "", rules: SmartRules(matchAll: true, rules: [], sort: .title, limit: nil))
        draft.rules = [SmartRuleDraft(kind: .artist)]
        return draft
    }

    public var smartRules: SmartRules {
        SmartRules(
            matchAll: matchAll,
            rules: rules.map(\.rule),
            sort: sort,
            limit: limited ? UInt32(clamping: max(1, limit)) : nil
        )
    }
}

/// One rule in the editor. Each kind keeps its own settings, so switching
/// a rule's kind and back doesn't lose what was typed.
public struct SmartRuleDraft: Identifiable, Equatable {
    public enum Kind: String, CaseIterable, Identifiable {
        case title, artist, album, genre, plays, length, added, lastPlayed, favourite
        public var id: String { rawValue }

        public var label: String {
            switch self {
            case .title: "Title"
            case .artist: "Artist"
            case .album: "Album"
            case .genre: "Genre"
            case .plays: "Plays"
            case .length: "Length"
            case .added: "Date Added"
            case .lastPlayed: "Last Played"
            case .favourite: "Favourite"
            }
        }

        var textField: SmartTextField? {
            switch self {
            case .title: .title
            case .artist: .artist
            case .album: .album
            case .genre: .genre
            default: nil
            }
        }
    }

    public let id: UUID
    public var kind: Kind
    public var textTest: SmartTextTest = .contains
    public var text = ""
    public var numberTest: SmartNumberTest = .atLeast
    /// Plays, or minutes for a length.
    public var number: Double = 1
    public var dateTest: SmartDateTest = .inLast
    public var days = 30
    public var favourite = true

    public init(kind: Kind) {
        id = UUID()
        self.kind = kind
        if kind == .length { number = 5 }
    }

    public init(_ rule: SmartRule) {
        id = UUID()
        switch rule {
        case .text(let field, let test, let value):
            switch field {
            case .title: kind = .title
            case .artist: kind = .artist
            case .album: kind = .album
            case .genre: kind = .genre
            }
            textTest = test
            text = value
        case .number(let field, let test, let value):
            kind = field == .playCount ? .plays : .length
            numberTest = test
            number = field == .playCount ? value : value / 60
        case .date(let field, let test, let days):
            kind = field == .added ? .added : .lastPlayed
            dateTest = test
            self.days = Int(days)
        case .favourite(let favourite):
            kind = .favourite
            self.favourite = favourite
        }
    }

    public var rule: SmartRule {
        if let field = kind.textField {
            return .text(field: field, test: textTest, value: text)
        }
        switch kind {
        case .plays:
            return .number(field: .playCount, test: numberTest, value: number)
        case .length:
            return .number(field: .lengthSeconds, test: numberTest, value: number * 60)
        case .added:
            return .date(field: .added, test: dateTest, days: UInt32(clamping: max(1, days)))
        case .lastPlayed:
            return .date(field: .lastPlayed, test: dateTest, days: UInt32(clamping: max(1, days)))
        default:
            return .favourite(favourite: favourite)
        }
    }
}
