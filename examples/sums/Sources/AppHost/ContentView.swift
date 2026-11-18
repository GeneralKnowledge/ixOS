@preconcurrency import SumsFFI
@preconcurrency import ForgeKit
import ForgeUI
import SwiftUI

/// The app's root. ForgeKit's `ForgeRoot` shows a spinner while the Rust
/// core opens, the calculator once it has, or a recovery screen if it failed.
public struct ContentView: View {
    @Bindable private var host: ForgeHost<SumsCore>

    public init(host: ForgeHost<SumsCore>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            CalculatorScreen(core: core, changes: host.changes)
        }
    }
}

/// The calculator: the sum above, the number below it, and twenty keys.
/// Its look is its own; only the behaviour follows the reference.
private struct CalculatorScreen: View {
    @State private var model: SumsModel
    @Environment(\.forgeTheme) private var theme
    let changes: UInt64

    init(core: SumsCore, changes: UInt64) {
        _model = State(initialValue: SumsModel(core: core))
        self.changes = changes
    }

    var body: some View {
        VStack(spacing: 12) {
            Spacer(minLength: 0)
            display
            keypad
        }
        .padding()
        .background(theme.colors.background.ignoresSafeArea())
        // Rust committed a change, or the app came back: show what Rust holds.
        .onChange(of: changes) { model.reload() }
        .alert(
            "Something Went Wrong",
            isPresented: Binding(
                get: { model.errorMessage != nil },
                set: { if !$0 { model.errorMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.errorMessage ?? "")
        }
    }

    private var display: some View {
        VStack(alignment: .trailing, spacing: 4) {
            Text(model.screen.expression ?? " ")
                .font(.title2)
                .foregroundStyle(theme.colors.textSecondary)
                .accessibilityIdentifier("sums.expression")
                .accessibilityHidden(model.screen.expression == nil)
            Text(model.screen.display)
                .font(.system(size: 72, weight: .light, design: .rounded))
                .monospacedDigit()
                .foregroundStyle(theme.colors.textPrimary)
                .accessibilityIdentifier("sums.display")
        }
        .lineLimit(1)
        .minimumScaleFactor(0.3)
        .frame(maxWidth: .infinity, alignment: .trailing)
    }

    private var keypad: some View {
        Grid(horizontalSpacing: 10, verticalSpacing: 10) {
            ForEach(SumsKeyFace.rows.indices, id: \.self) { row in
                GridRow {
                    ForEach(SumsKeyFace.rows[row], id: \.name) { face in
                        key(face)
                    }
                }
            }
        }
    }

    private func key(_ face: SumsKeyFace) -> some View {
        // The clear key shows C while there is an entry to clear, AC after.
        let clearsEntry = face.key == .clear && model.screen.clearsEntry
        return Button {
            Task { await model.press(face.key) }
        } label: {
            Text(clearsEntry ? "C" : face.symbol)
                .font(.title.weight(.medium))
                .frame(maxWidth: .infinity, minHeight: 56)
        }
        .buttonStyle(ForgeButtonStyle(style(face.kind), expands: true))
        .accessibilityLabel(clearsEntry ? "Clear" : face.label)
        .accessibilityIdentifier(face.identifier)
    }

    private func style(_ kind: SumsKeyFace.Kind) -> ForgeButtonStyle.Kind {
        switch kind {
        case .operation: .primary
        case .function: .tertiary
        case .digit: .secondary
        }
    }
}
