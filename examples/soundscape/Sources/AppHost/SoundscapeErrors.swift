@preconcurrency import SoundscapeFFI
@preconcurrency import ForgeKit
import Foundation

/// Rust's errors in the person's words. Foundation only, so it tests on
/// Linux.
public enum SoundscapeErrors {
    nonisolated public static func describe(_ error: SoundscapeError) -> String {
        switch error {
        case .EmptyName: return "Give it a name."
        case .NameTooLong(let max): return "Names can be up to \(max) characters."
        case .OutOfRange: return "That is out of range."
        case .Audio: return "That file can't be used."
        case .InUse: return "Soundscape is open in another copy of the app."
        case .Corrupt: return "Your music library couldn't be read."
        case .NewerData: return "Your library was saved by a newer version of Soundscape."
        case .Storage: return "Couldn't save. Try again."
        case .BadAddress(let reason): return sentence(reason)
        case .Unreachable(let reason):
            // The system's own reason goes last, so a cause on the phone
            // (such as App Transport Security) is visible, not just a guess.
            return "Can't reach the take server. Check that it's running on the computer and that this phone is on Tailscale (or the same network)."
                + (reason.isEmpty ? "" : "\n\n" + sentence(reason))
        case .Server(let reason): return sentence(reason)
        case .NotInLibrary: return "That isn't in your library any more. Pull down to refresh it."
        case .Stopped: return "Stopped."
        }
    }

    /// Rust's reasons start lower case; the screen shows a sentence.
    nonisolated static func sentence(_ text: String) -> String {
        guard let first = text.first else { return text }
        let capital = first.uppercased() + text.dropFirst()
        return capital.hasSuffix(".") || capital.hasSuffix("?") ? capital : capital + "."
    }

    nonisolated public static func title(_ error: SoundscapeError) -> String {
        switch error {
        case .EmptyName, .NameTooLong, .OutOfRange: return "Check the Details"
        case .Audio: return "File Not Used"
        case .InUse: return "Already Open"
        case .Corrupt, .NewerData, .Storage: return "Couldn't Save"
        case .BadAddress: return "Check the Address"
        case .Unreachable: return "Can't Reach the Server"
        case .Server: return "The Server Said No"
        case .NotInLibrary: return "Not in the Library"
        case .Stopped: return "Stopped"
        }
    }
}

extension SoundscapeError: ForgeDescribedError {
    public var failure: ForgeFailure {
        let retryable: Bool
        switch self {
        case .Storage, .Unreachable: retryable = true
        default: retryable = false
        }
        return ForgeFailure(
            title: SoundscapeErrors.title(self),
            message: SoundscapeErrors.describe(self),
            isRetryable: retryable
        )
    }

    public var isCancellation: Bool {
        if case .Stopped = self { return true }
        return false
    }
}
