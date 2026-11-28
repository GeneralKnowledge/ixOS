import Foundation

/// The text in a note's editor, and whether it is a draft an earlier save
/// failed to keep (`NotesModel.unsavedText`). Kept apart from SwiftUI so the
/// rules are tested on Linux.
///
/// A save that fails after the editor opened leaves a draft the editor
/// didn't load. The editor shows it only if the person hasn't changed the
/// text, and drops a draft only if it is the one this editor shows, so no
/// failed save loses its text.
public struct NoteEditorText: Equatable, Sendable {
    /// What the editor shows; the text field writes here.
    public var text: String
    /// The retained draft this editor loaded, if any.
    public private(set) var loadedDraft: String?

    /// What leaving the editor does.
    public enum Leave: Equatable, Sendable {
        /// Nothing changed (or the text is blank, which Rust refuses).
        case nothing
        /// Back to the saved text: drop the draft this editor loaded.
        case discardDraft
        /// Save this text.
        case save(String)
    }

    /// Opens on the retained draft if there is one, else the saved text.
    public init(saved: String, retained: String?) {
        text = retained ?? saved
        loadedDraft = retained
    }

    /// A save failed while the editor was open and left `retained`. Shown
    /// only if the person hasn't changed the text (it is still the saved
    /// text or the draft it loaded); newer edits are kept and saved instead.
    public mutating func retainedChanged(_ retained: String?, saved: String) {
        guard let retained, retained != loadedDraft,
            text == saved || text == loadedDraft
        else { return }
        text = retained
        loadedDraft = retained
    }

    /// Use Saved Version: the draft is dropped and the saved text shown.
    public mutating func useSaved(_ saved: String) {
        text = saved
        loadedDraft = nil
    }

    /// What to do on leaving, given what Rust holds now and the draft the
    /// model keeps.
    public func leaving(saved: String, retained: String?) -> Leave {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return .nothing }
        if trimmed != saved { return .save(trimmed) }
        if let retained, retained == loadedDraft { return .discardDraft }
        return .nothing
    }
}
