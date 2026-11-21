// What a rename sheet shows when the app refuses a name. Foundation only,
// so the rule is checked on Linux; ForgeRename+AppKit.swift uses it.

import Foundation

/// Turns a throwing rename into what ForgeUI's rename sheet expects: nil
/// when the name was saved, otherwise the sentence to show under the field.
public enum ForgeRenameRefusal {
    /// Runs `save`. Nil when it saved, or when the work was stopped on
    /// purpose (stopped work is not a refusal and shows nothing); otherwise
    /// the error's `ForgeFailure` message, in the app's own words.
    public static func reason(_ save: () async throws -> Void) async -> String? {
        do {
            try await save()
            return nil
        } catch {
            if ForgeFailure.isCancellation(error) { return nil }
            return ForgeFailure(error).message
        }
    }
}
