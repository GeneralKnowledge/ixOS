import CounterBridge
import SwiftUI

public struct ContentView: View {
    public init() {}
    @StateObject private var model = CounterViewModel()

    public var body: some View {
        Group {
            if model.isLoading {
                ProgressView("Opening…")
            } else if model.isBlocked {
                blockedView
            } else {
                counterView
            }
        }
        .padding()
        // Opens Rust off the main thread. `open()` is idempotent, so the view
        // reappearing does not open a second instance or subscription.
        .task { await model.open() }
    }

    private var counterView: some View {
        VStack(spacing: 24) {
            Text("\(model.count)")
                .font(.system(size: 72, weight: .bold, design: .rounded))
                .accessibilityLabel("Count")
                .accessibilityValue("\(model.count)")

            HStack(spacing: 16) {
                Button("Decrement") { Task { await model.decrement() } }
                    .buttonStyle(.bordered)
                Button("Increment") { Task { await model.increment() } }
                    .buttonStyle(.borderedProminent)
            }

            Button("Reset", role: .destructive) { Task { await model.reset() } }

            bulkAddSection

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

            Text(CounterClient.greeting)
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
    }

    /// A long-running Rust operation: progress, cancellation, completion.
    @ViewBuilder
    private var bulkAddSection: some View {
        if let progress = model.bulkAddProgress {
            VStack(spacing: 8) {
                ProgressView(value: progress) { Text("Adding 100…") }
                Button("Cancel", role: .cancel) { model.cancelBulkAdd() }
            }
            .frame(maxWidth: 240)
        } else {
            Button("Add 100 slowly") { model.startBulkAdd() }
                .buttonStyle(.bordered)
        }
        if let message = model.lastOperationMessage {
            Text(message)
                .font(.footnote)
                .foregroundStyle(.secondary)
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
            Button("Retry") { Task { await model.retryOpen() } }
                .buttonStyle(.borderedProminent)
            if model.canRecoverFromCorruption {
                Button("Reset storage and start fresh", role: .destructive) {
                    Task { await model.recoverFromCorruption() }
                }
            }
        }
    }
}
