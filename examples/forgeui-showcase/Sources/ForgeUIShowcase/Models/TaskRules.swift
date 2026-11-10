import ForgeUI
import Foundation

/// What the person has typed into the task form.
public struct TaskDraft: Equatable, Sendable {
    public var title: String
    public var notes: String

    public init(title: String = "", notes: String = "") {
        self.title = title
        self.notes = notes
    }
}

/// The task form's rules, turned into ForgeUI field messages.
///
/// In an ixOS app a rule like this belongs in Rust, and the model only maps
/// Rust's answer to `ForgeFieldMessage` (Pocket Library, the reference app,
/// shows that). The showcase keeps it in Swift so that it stays an ordinary
/// SwiftUI app with no Rust core: ForgeUI doesn't care where a message
/// comes from.
public enum TaskRules {
    public static let maxTitle = 80
    public static let longTitle = 60
    public static let maxNotes = 500

    /// The title's message: an error that blocks saving, a warning, or nil.
    /// A blank title is only reported once the person has typed in the field.
    public static func titleMessage(_ draft: TaskDraft, touched: Bool) -> ForgeFieldMessage? {
        let title = draft.title.trimmingCharacters(in: .whitespacesAndNewlines)
        if title.isEmpty {
            return touched ? .error("Give the task a title.") : nil
        }
        if title.count > maxTitle {
            return .error("Titles can be up to \(maxTitle) characters. This one is \(title.count).")
        }
        if title.count > longTitle {
            return .warning("Long titles are cut short in the list. Put details in the notes.")
        }
        return nil
    }

    public static func notesMessage(_ draft: TaskDraft) -> ForgeFieldMessage {
        let count = draft.notes.trimmingCharacters(in: .whitespacesAndNewlines).count
        if count > maxNotes {
            return .error("Notes can be up to \(maxNotes) characters. These are \(count).")
        }
        return .help("\(count) of \(maxNotes) characters")
    }

    public static func canSave(_ draft: TaskDraft) -> Bool {
        titleMessage(draft, touched: true)?.isError != true && !notesMessage(draft).isError
    }
}
