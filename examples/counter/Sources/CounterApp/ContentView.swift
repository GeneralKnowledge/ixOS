import SwiftUI

struct ContentView: View {
    @StateObject private var model = CounterViewModel()

    var body: some View {
        Group {
            if model.isBlocked {
                blockedView
            } else {
                counterView
            }
        }
        .padding()
    }

    private var counterView: some View {
        VStack(spacing: 24) {
            Text("\(model.count)")
                .font(.system(size: 72, weight: .bold, design: .rounded))
                .accessibilityLabel("Count")
                .accessibilityValue("\(model.count)")

            HStack(spacing: 16) {
                Button("Decrement") { model.decrement() }
                    .buttonStyle(.bordered)
                Button("Increment") { model.increment() }
                    .buttonStyle(.borderedProminent)
            }

            Button("Reset", role: .destructive) { model.reset() }

            if !model.startupLabel.isEmpty {
                Text(model.startupLabel)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            if let message = model.errorMessage {
                Text(message)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }

            Text(forgeGreeting())
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
    }

    private var blockedView: some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.largeTitle)
                .foregroundStyle(.orange)
            Text("Counter unavailable")
                .font(.headline)
            if let message = model.errorMessage {
                Text(message)
                    .font(.footnote)
                    .multilineTextAlignment(.center)
            }
            Button("Retry") { model.retryOpen() }
                .buttonStyle(.borderedProminent)
            if model.canRecoverFromCorruption {
                Button("Reset storage and start fresh", role: .destructive) {
                    model.recoverFromCorruption()
                }
            }
        }
    }
}
