import ForgeAppKit
import ForgeUI
import SwiftUI

/// ForgeAppKit's shared behaviours in one place, each variant one tap away,
/// so a change to them can be seen on the phone before an app uses it. The
/// visual components are in the ForgeUI Showcase.
struct PatternsView: View {
    @State private var banner: ForgeBanner<PatternUndo>?
    @State private var feedback = ForgeFeedback()
    @State private var outcome = "Nothing yet"
    @State private var sending: Task<Void, Never>?

    var body: some View {
        List {
            Section {
                Button("Delete with Undo") {
                    outcome = "“Buy milk” deleted"
                    feedback.play(.success)
                    banner = .undo("Deleted “Buy milk”", action: .restore, identifier: "patterns.undo")
                }
                .accessibilityIdentifier("patterns.banner.undo")
                Button("Work with Progress") { startSending() }
                    .accessibilityIdentifier("patterns.banner.progress")
                Button("Work Without Progress") {
                    banner = .working("Checking for new articles")
                    Task {
                        try? await Task.sleep(for: .seconds(3))
                        banner = ForgeBanner("3 new articles")
                        feedback.play(.refresh)
                    }
                }
                .accessibilityIdentifier("patterns.banner.working")
                Button("Message") {
                    banner = ForgeBanner("Saved to Files")
                    feedback.play(.success)
                }
                .accessibilityIdentifier("patterns.banner.message")
                LabeledContent("Last action", value: outcome)
                    .accessibilityIdentifier("patterns.outcome")
            } header: {
                Text("Banner")
            } footer: {
                Text("For work that continues, and Undo after a frequent, cheap action (Checklist's delete, Commonplace's Mark All as Read). Errors that need a decision stay alerts. Swipe a banner down to dismiss it.")
            }

            Section {
                ForEach(ForgeFeedback.Kind.allCases, id: \.self) { kind in
                    Button {
                        feedback.play(kind)
                        // A failure is never felt without being explained.
                        if kind == .failure {
                            outcome = "Couldn't send: the other device went offline"
                        }
                    } label: {
                        LabeledContent(kind.rawValue.capitalized, value: use(kind))
                    }
                    .accessibilityIdentifier("patterns.feedback.\(kind.rawValue)")
                }
            } header: {
                Text("Haptics")
            } footer: {
                Text("Proportional: a saved trip or a finished session gets one; a toggle or a routine tap gets none. The model calls feedback.play(.success); the screen has .forgeFeedback(model.feedback).")
            }
        }
        .navigationTitle("Interaction Patterns")
        .forgeBanner($banner) { action in
            switch action {
            case .restore: outcome = "“Buy milk” restored"
            }
        }
        .forgeFeedback(feedback)
        .onDisappear { sending?.cancel() }
    }

    private func use(_ kind: ForgeFeedback.Kind) -> String {
        switch kind {
        case .success: "Saved, sent, file arrived"
        case .failure: "With the message why"
        case .selection: "A step or a rating"
        case .refresh: "Message in another chat"
        }
    }

    private func startSending() {
        sending?.cancel()
        let started = ForgeBanner<PatternUndo>.working("Sending 3 photos", progress: 0)
        banner = started
        sending = Task {
            for step in 1...10 {
                try? await Task.sleep(for: .milliseconds(400))
                if Task.isCancelled || banner?.id != started.id { return }
                banner = started.updated(message: "Sending 3 photos", progress: Double(step) / 10)
            }
            banner = ForgeBanner("Sent 3 photos")
            feedback.play(.success)
        }
    }
}

enum PatternUndo {
    case restore
}
