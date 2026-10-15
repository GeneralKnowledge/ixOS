import SwiftUI

struct ContentView: View {
    @StateObject private var model = CounterViewModel()

    var body: some View {
        VStack(spacing: 24) {
            Text("\(model.count)")
                .font(.system(size: 72, weight: .bold, design: .rounded))
                .accessibilityLabel("Count")
                .accessibilityValue("\(model.count)")

            HStack(spacing: 16) {
                Button("Decrement") { model.decrement() }
                    .buttonStyle(.bordered)
                    .accessibilityLabel("Decrement")
                Button("Increment") { model.increment() }
                    .buttonStyle(.borderedProminent)
                    .accessibilityLabel("Increment")
            }

            Button("Reset", role: .destructive) { model.reset() }
                .accessibilityLabel("Reset counter")

            HStack {
                Image(systemName: model.persistenceOk ? "externaldrive.fill.badge.checkmark" : "externaldrive.badge.exclamationmark")
                Text(model.persistenceOk ? "Saved" : "Save issue")
                    .font(.footnote)
                    .foregroundStyle(model.persistenceOk ? .secondary : .orange)
            }
            .accessibilityElement(children: .combine)

            if let message = model.errorMessage {
                Text(message)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
                    .accessibilityLabel("Error")
                    .accessibilityValue(message)
            }

            Text(forgeGreeting())
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding()
    }
}
