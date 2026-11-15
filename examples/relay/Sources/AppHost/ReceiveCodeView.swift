#if os(iOS)
@preconcurrency import RelayFFI
import ForgeUI
import SwiftUI

/// Receive a file with a word code from a sender on this Wi-Fi (Relay M1).
/// The sender, for now, is `relay-spike send <file>` on a computer; it
/// prints a code such as `37-grasp-gag-drum`. Bonjour finds it by the
/// code's number, Rust does the rest.
struct ReceiveCodeSheet: View {
    @Bindable var model: RelayModel
    let network: RelayNetwork
    @Binding var preview: URL?

    @State private var code = ""
    @Environment(\.dismiss) private var dismiss
    @FocusState private var typing: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("7-acid-tidy-rug", text: $code)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.asciiCapable)
                        .submitLabel(.go)
                        .focused($typing)
                        .onSubmit(start)
                        .disabled(model.isReceivingCode)
                        .accessibilityLabel("Code")
                        .accessibilityIdentifier("receiveCode.code")
                } header: {
                    Text("Code")
                } footer: {
                    Text("The number and three words the sender shows. Both devices need to be on the same Wi-Fi.")
                }
                Section {
                    status
                }
            }
            .navigationTitle("Receive with a Code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(model.isReceivingCode ? "Stop" : "Done") {
                        if model.isReceivingCode {
                            model.cancelCodeReceive()
                        } else {
                            dismiss()
                        }
                    }
                    .accessibilityIdentifier("receiveCode.close")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Receive", action: start)
                        .disabled(model.isReceivingCode || code.trimmingCharacters(in: .whitespaces).isEmpty)
                        .accessibilityIdentifier("receiveCode.start")
                }
            }
            // The size of what is on its way, twice a second.
            .task(id: model.isReceivingCode) {
                while model.isReceivingCode, (try? await Task.sleep(for: .milliseconds(500))) != nil {
                    model.refreshCodeReceive()
                }
            }
            .onAppear {
                model.resetCodeReceive()
                typing = true
            }
            .interactiveDismissDisabled(model.isReceivingCode)
        }
    }

    @ViewBuilder private var status: some View {
        switch model.codeReceive {
        case .idle:
            Text("Type the code, then Receive.")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("receiveCode.status")
        case .finding:
            Label {
                Text("Looking for the sender…")
            } icon: {
                ProgressView()
            }
            .accessibilityIdentifier("receiveCode.status")
        case .receiving(let size, let since):
            VStack(alignment: .leading, spacing: 6) {
                Label {
                    if let size {
                        Text("Receiving \(RelayFormat.size(size.total))")
                    } else {
                        Text("Checking the code…")
                    }
                } icon: {
                    ProgressView()
                }
                if let size, size.already > 0 {
                    Text("Resuming: \(RelayFormat.size(size.already)) already here.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                TimelineView(.periodic(from: since, by: 1)) { context in
                    Text("\(Int(context.date.timeIntervalSince(since))) s")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("receiveCode.status")
        case .received(let receipt):
            VStack(alignment: .leading, spacing: 6) {
                Label("Received \(receipt.name)", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                Text(Self.summary(receipt))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button("Open", systemImage: "doc.text.magnifyingglass") {
                    preview = URL(fileURLWithPath: receipt.path)
                }
                .accessibilityIdentifier("receiveCode.open")
            }
            .accessibilityIdentifier("receiveCode.status")
        case .failed(let reason):
            Label(reason, systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
                .accessibilityIdentifier("receiveCode.status")
        }
    }

    private func start() {
        guard !model.isReceivingCode else { return }
        // A mistyped code fails at once, with the reason, before any search.
        typing = false
        let typed = code
        Task {
            await model.receive(code: typed) { number in
                await network.findCodeSender(number: number)
            }
        }
    }

    /// "12.3 MB in 4.1 s, 3.0 MB/s (2.0 MB were already here)".
    static func summary(_ receipt: CodeReceipt) -> String {
        var text = "\(RelayFormat.size(receipt.size)) in \(String(format: "%.1f", receipt.seconds)) s, "
            + "\(String(format: "%.1f", receipt.megabytesPerSecond)) MB/s"
        if receipt.resumedFrom > 0 {
            text += " (\(RelayFormat.size(receipt.resumedFrom)) were already here)"
        }
        return text
    }
}
#endif
