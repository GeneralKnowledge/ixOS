@preconcurrency import SumsFFI
import Foundation
import Observation

/// What the calculator screen shows. It holds no rules of its own: every key
/// goes to the Rust `SumsCore` (rust/src/engine.rs has the rules), and the
/// screen shows what Rust returns.
///
/// It imports no SwiftUI, so `forge check swift` compiles and tests it on
/// Linux.
@Observable
@MainActor
public final class SumsModel {
    public private(set) var screen: SumsScreen
    /// A failed key press, for an alert. The view clears it when dismissed.
    public var errorMessage: String?

    public let core: SumsCore

    public init(core: SumsCore) {
        self.core = core
        screen = core.screen()
    }

    /// Re-reads the screen from Rust. Cheap: it never waits for a command.
    public func reload() {
        screen = core.screen()
    }

    public func press(_ key: SumsKey) async {
        do {
            screen = try await core.press(key: key)
        } catch {
            errorMessage = Self.describe(error)
        }
    }

    /// Text for the alert. `reason` fields are diagnostics for the log,
    /// never the screen.
    public static func describe(_ error: Error) -> String {
        guard let error = error as? SumsError else { return error.localizedDescription }
        switch error {
        case .Storage: return "The sum couldn't be saved. Try again, or close the app and open it again."
        case .InUse: return "Sums is open in another copy of the app."
        case .ShutDown: return "The app is closing."
        }
    }
}

/// How each key is shown and found. The identifiers are `sums.key.<name>`,
/// where the name is the reference calculator's id in lower case
/// (`Seven` -> `sums.key.seven`), so one sequence table drives both apps
/// (docs/research/expeditions/calculator/run_sequences.py).
public struct SumsKeyFace: Sendable {
    public let key: SumsKey
    /// What the key shows.
    public let symbol: String
    /// What VoiceOver says.
    public let label: String
    public let name: String

    public var identifier: String { "sums.key.\(name)" }

    public enum Kind: Sendable { case digit, operation, function }
    public var kind: Kind {
        switch key {
        case .add, .subtract, .multiply, .divide, .equals: .operation
        case .clear, .negate, .percent, .delete: .function
        default: .digit
        }
    }

    /// The keypad, row by row.
    public static let rows: [[SumsKeyFace]] = [
        [face(.clear, "AC", "All Clear", "clear"), face(.negate, "±", "Change Sign", "negate"),
         face(.percent, "%", "Percent", "percent"), face(.divide, "÷", "Divide", "divide")],
        [digit(.seven, 7), digit(.eight, 8), digit(.nine, 9), face(.multiply, "×", "Multiply", "multiply")],
        [digit(.four, 4), digit(.five, 5), digit(.six, 6), face(.subtract, "−", "Subtract", "subtract")],
        [digit(.one, 1), digit(.two, 2), digit(.three, 3), face(.add, "+", "Add", "add")],
        [face(.delete, "⌫", "Delete", "delete"), digit(.zero, 0),
         face(.decimal, ".", "Decimal Point", "decimal"), face(.equals, "=", "Equals", "equals")],
    ]

    private static func face(_ key: SumsKey, _ symbol: String, _ label: String, _ name: String) -> SumsKeyFace {
        SumsKeyFace(key: key, symbol: symbol, label: label, name: name)
    }

    private static func digit(_ key: SumsKey, _ value: Int) -> SumsKeyFace {
        let names = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]
        return SumsKeyFace(key: key, symbol: "\(value)", label: "\(value)", name: names[value])
    }
}
