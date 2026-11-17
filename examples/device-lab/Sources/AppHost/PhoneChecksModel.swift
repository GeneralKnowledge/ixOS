import Foundation
import Observation

/// The Checks screen (Tools tab): the checks for the next phone sitting,
/// built into the app (`PhoneChecks.text`), so they are read and ticked on
/// the phone itself and the results shared as text in one go. No UI
/// imports, so it tests on Linux.
@MainActor
@Observable
public final class PhoneChecksModel {
    public struct Item: Identifiable, Equatable, Sendable {
        /// The check's text, which is also its key: a check reworded for a
        /// later build starts unanswered.
        public let text: String
        public var id: String { text }
    }

    public struct Group: Identifiable, Equatable, Sendable {
        public let title: String
        public let items: [Item]
        public var id: String { title }
    }

    public enum Result: String, Codable, Sendable {
        case pass, fail, skipped
    }

    public struct Answer: Codable, Equatable, Sendable {
        public var result: Result?
        public var note: String = ""
    }

    public let title: String
    public let groups: [Group]
    public private(set) var answers: [String: Answer]

    @ObservationIgnored private let build: String
    @ObservationIgnored private let defaults: UserDefaults

    /// `text` is a short Markdown list: `# title`, `## group`, then items
    /// as `1. …` or `- …`; other lines are ignored. Answers are kept per
    /// build, so a new build starts with every check unanswered.
    public init(text: String, build: String, defaults: UserDefaults = .standard) {
        var title = "Phone Checks"
        var groups: [Group] = []
        var current: (String, [Item])?
        func close() {
            if let (name, items) = current, !items.isEmpty { groups.append(Group(title: name, items: items)) }
        }
        for raw in text.split(separator: "\n", omittingEmptySubsequences: true) {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("## ") {
                close()
                current = (String(line.dropFirst(3)), [])
            } else if line.hasPrefix("# ") {
                title = String(line.dropFirst(2))
            } else if let item = Self.item(line) {
                if current == nil { current = ("Checks", []) }
                current?.1.append(Item(text: item))
            }
        }
        close()
        self.title = title
        self.groups = groups
        self.build = build
        self.defaults = defaults
        answers = (defaults.data(forKey: Self.key(build)))
            .flatMap { try? JSONDecoder().decode([String: Answer].self, from: $0) } ?? [:]
    }

    public func answer(_ item: Item) -> Answer { answers[item.id] ?? Answer() }

    public func set(_ result: Result?, for item: Item) {
        var answer = answer(item)
        answer.result = answer.result == result ? nil : result
        answers[item.id] = answer
        save()
    }

    public func setNote(_ note: String, for item: Item) {
        var answer = answer(item)
        answer.note = note
        answers[item.id] = answer
        save()
    }

    /// "5 of 12 answered, 1 failed".
    public var progress: String {
        let all = groups.flatMap(\.items)
        let answered = all.filter { answer($0).result != nil }.count
        let failed = all.filter { answer($0).result == .fail }.count
        return "\(answered) of \(all.count) answered" + (failed > 0 ? ", \(failed) failed" : "")
    }

    /// The results as plain text for the share sheet, failures first in
    /// each group's own order.
    public var shareText: String {
        var out = "\(title), build \(build)\n"
        for group in groups {
            out += "\n\(group.title)\n"
            for item in group.items {
                let answer = answer(item)
                let mark = switch answer.result {
                case .pass: "PASS"
                case .fail: "FAIL"
                case .skipped: "SKIP"
                case nil: "----"
                }
                out += "\(mark) \(item.text)"
                let note = answer.note.trimmingCharacters(in: .whitespacesAndNewlines)
                if !note.isEmpty { out += "\n     Note: \(note)" }
                out += "\n"
            }
        }
        return out
    }

    public func clear() {
        answers = [:]
        save()
    }

    private func save() {
        if let data = try? JSONEncoder().encode(answers) {
            defaults.set(data, forKey: Self.key(build))
        }
    }

    private static func key(_ build: String) -> String { "phoneChecks.\(build)" }

    /// `1. text`, `12) text` or `- text`, without the marker.
    private static func item(_ line: String) -> String? {
        if line.hasPrefix("- ") || line.hasPrefix("* ") { return String(line.dropFirst(2)) }
        let digits = line.prefix { $0.isNumber }
        guard !digits.isEmpty else { return nil }
        let rest = line.dropFirst(digits.count)
        guard rest.hasPrefix(". ") || rest.hasPrefix(") ") else { return nil }
        return String(rest.dropFirst(2))
    }
}
