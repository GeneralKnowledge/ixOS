@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// An exercise followed one card at a time: "get ready", each step, then
/// how it went. Large text, and big Back and Next buttons that are easy to
/// hit while holding a lead. Rust (`GuidedSession`) decides which card
/// shows and what is allowed; this view only presents it. Closing it part
/// way records nothing.
struct GuidedSessionView: View {
    let model: GoodDogModel
    @Bindable var session: GuidedSessionModel
    @Binding var timerStartedAt: Date?
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var showingIfDifficult = false
    @State private var result: SessionResult?
    @State private var welfareConcern = false
    @State private var saving = false
    @State private var feedback: SessionFeedback?
    @AccessibilityFocusState private var headingFocused: Bool

    private var card: GuidedCard { session.card }

    var body: some View {
        NavigationStack {
            Group {
                if card.kind == .outcome {
                    outcomeCard
                } else {
                    textCard
                }
            }
            .safeAreaInset(edge: .top, spacing: 0) {
                SessionTimer(startedAt: $timerStartedAt, screen: "guided")
                    .padding(.horizontal)
                    .padding(.vertical, 8)
                    .background(.bar)
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                controls
                    .padding()
                    .background(.bar)
            }
            .navigationTitle(session.title)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { dismiss() }
                        .accessibilityLabel("Close guided session")
                        .accessibilityHint(card.recorded ? "" : "Nothing is recorded.")
                        .accessibilityIdentifier("guided.close")
                }
            }
            .sheet(isPresented: $showingIfDifficult) {
                IfDifficultSheet(model: model, text: session.ifDifficult)
            }
            .alert(
                "Session Saved",
                isPresented: Binding(get: { feedback != nil }, set: { if !$0 { feedback = nil } }),
                presenting: feedback
            ) { _ in
                Button("OK", role: .cancel) { dismiss() }
                    .accessibilityIdentifier("feedback.ok")
            } message: { feedback in
                Text(feedback.message)
            }
            .errorAlert("Couldn't Save", message: $session.error)
            .sensoryFeedback(.selection, trigger: card.position)
            .onChange(of: card.position) { headingFocused = true }
            // Glossary links open here: the tabs underneath can't present
            // while this session covers them.
            .termSheet(model: model)
        }
    }

    // MARK: Cards

    private var heading: some View {
        Text(card.heading)
            .font(.largeTitle.bold())
            .accessibilityAddTraits(.isHeader)
            .accessibilityFocused($headingFocused)
            .accessibilityIdentifier("guided.progress")
    }

    /// "Get ready" or one step, in large text.
    private var textCard: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                heading
                if let number = card.stepNumber {
                    ProgressView(value: Double(number), total: Double(max(card.stepCount, 1)))
                        .accessibilityHidden(true)  // the heading says it
                }
                LinkedTextView(text: card.text)
                    .font(.title2)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("guided.text")
                if card.kind == .getReady, let safety = session.safetyNote {
                    Label {
                        LinkedTextView(text: safety)
                    } icon: {
                        Image(systemName: "heart.text.square")
                            .forgeForeground(.warning)
                    }
                    .font(.title3)
                }
                Button {
                    showingIfDifficult = true
                } label: {
                    Label("If It's Difficult", systemImage: "questionmark.circle")
                        .font(.title3)
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.bordered)
                .accessibilityHint("Shows what to do if it's hard. You stay on this card.")
                .accessibilityIdentifier("guided.ifDifficult")
            }
            .padding()
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        // A new card starts at its top.
        .id(card.position)
    }

    /// After the last step: what to look for, then the same outcome choice
    /// as "How Did It Go?".
    private var outcomeCard: some View {
        Form {
            Section {
                heading
                    .listRowSeparator(.hidden)
                LinkedTextView(text: card.text)
                    .font(.title3)
                    .accessibilityIdentifier("guided.text")
            } footer: {
                Text("What to look for")
            }
            OutcomeChoice(title: session.title, result: $result, welfareConcern: $welfareConcern)
                .disabled(!card.canRecord)
        }
        .sensoryFeedback(.selection, trigger: result)
    }

    // MARK: Controls

    /// Back and Next (Save on the last card), side by side, or one above the
    /// other at the largest text sizes.
    @ViewBuilder private var controls: some View {
        if typeSize.isAccessibilitySize {
            VStack(spacing: 12) {
                forward
                back
            }
        } else {
            HStack(spacing: 12) {
                back
                forward
            }
        }
    }

    private var back: some View {
        Button {
            session.back()
        } label: {
            Label("Back", systemImage: "chevron.backward")
                .font(.title3.bold())
                .frame(maxWidth: .infinity, minHeight: 56)
        }
        .buttonStyle(.bordered)
        .disabled(!card.canGoBack || saving)
        .accessibilityLabel(card.position == 1 ? "Back to get ready" : "Back")
        .accessibilityIdentifier("guided.back")
    }

    @ViewBuilder private var forward: some View {
        if card.kind == .outcome {
            Button {
                Task { await save() }
            } label: {
                Label("Save", systemImage: "checkmark")
                    .font(.title3.bold())
                    .frame(maxWidth: .infinity, minHeight: 56)
            }
            .buttonStyle(.borderedProminent)
            .disabled(result == nil || saving || !card.canRecord)
            .accessibilityLabel("Save session")
            .accessibilityHint(result == nil ? "Choose how it went first." : "")
            .accessibilityIdentifier("guided.save")
        } else {
            Button {
                session.next()
            } label: {
                Label("Next", systemImage: "chevron.forward")
                    .labelStyle(TrailingIconLabelStyle())
                    .font(.title3.bold())
                    .frame(maxWidth: .infinity, minHeight: 56)
            }
            .buttonStyle(.borderedProminent)
            .disabled(!card.canGoNext)
            .accessibilityLabel(nextLabel)
            .accessibilityIdentifier("guided.next")
        }
    }

    private var nextLabel: String {
        switch card.kind {
        case .getReady: "Next: step 1"
        case .step where card.stepNumber == card.stepCount: "Next: how did it go?"
        case .step: "Next: step \((card.stepNumber ?? 0) + 1)"
        case .outcome: "Next"
        }
    }

    private func save() async {
        guard let result else { return }
        saving = true
        defer { saving = false }
        do {
            feedback = try await session.record(result: result, welfareConcern: welfareConcern)
            timerStartedAt = nil  // the session is over
        } catch {
            session.error = GoodDogModel.describe(error)
        }
    }
}

/// "If it's difficult", over the card, so closing it returns to the same place.
private struct IfDifficultSheet: View {
    let model: GoodDogModel
    let text: LinkedText
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                LinkedTextView(text: text)
                    .font(.title3)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding()
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityIdentifier("guided.ifDifficult.text")
            }
            .navigationTitle("If It's Difficult")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityLabel("Back to the step")
                        .accessibilityIdentifier("guided.ifDifficult.done")
                }
            }
            .termSheet(model: model)
        }
        .presentationDetents([.medium, .large])
    }
}

/// Title first, then the icon: "Next ›".
private struct TrailingIconLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 6) {
            configuration.title
            configuration.icon
        }
    }
}
