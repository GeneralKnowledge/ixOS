@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// One exercise: why, how, what to look for, and what to do if it's hard.
/// "Start Guided Session" follows it one card at a time; "How Did It Go?"
/// records the session in Rust.
struct ExerciseView: View {
    let model: GoodDogModel
    let route: ExerciseRoute
    @State private var recording = false
    @State private var feedback: SessionFeedback?
    @State private var guided: GuidedSessionModel?
    @State private var startError: String?
    /// The session timer, shared with the guided session so it keeps running.
    @State private var timerStartedAt: Date?

    private var activity: PlanActivity? {
        model.today?.activities.first { $0.id == route.activityId }
    }

    var body: some View {
        if let exercise = model.exercise(id: route.exerciseId) {
            List {
                Section {
                    Text(exercise.summary)
                        .font(.headline)
                    LabeledContent("Time", value: "\(exercise.minutes) min")
                    LabeledContent("Skill", value: "\(exercise.skill.name): \(exercise.skill.level.title)")
                        .accessibilityIdentifier("exercise.skill")
                    if let activity {
                        Text(activity.whyToday)
                            .foregroundStyle(.secondary)
                    }
                    if activity?.result == nil {
                        Button {
                            startGuided()
                        } label: {
                            Label("Start Guided Session", systemImage: "figure.walk")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)
                        .accessibilityHint("Shows the steps one at a time, with large Back and Next buttons.")
                        .accessibilityIdentifier("exercise.guided")
                    }
                }
                Section("Why it helps") {
                    LinkedTextView(text: exercise.purpose)
                }
                Section("Get ready") {
                    LinkedTextView(text: exercise.preparation)
                }
                Section("Steps") {
                    ForEach(Array(exercise.steps.enumerated()), id: \.offset) { index, step in
                        HStack(alignment: .firstTextBaseline, spacing: 12) {
                            Text("\(index + 1)")
                                .font(.headline)
                                .foregroundStyle(.tint)
                                .accessibilityHidden(true)
                            LinkedTextView(text: step)
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityLabel("Step \(index + 1): \(step.plain)")
                        .accessibilityIdentifier("exercise.step.\(index)")
                    }
                }
                Section("Look for") {
                    LinkedTextView(text: exercise.lookFor)
                }
                Section("If it's difficult") {
                    LinkedTextView(text: exercise.ifDifficult)
                }
                if let safety = exercise.safetyNote {
                    Section {
                        Label {
                            LinkedTextView(text: safety)
                        } icon: {
                            Image(systemName: "heart.text.square")
                                .forgeForeground(.warning)
                        }
                    } header: {
                        Text("Keep it safe")
                    }
                }
                Section {
                    Label(exercise.hint, systemImage: "lightbulb")
                    if let why = exercise.whyItWorks {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(why.text)
                            if let term = why.term {
                                Text("The word for this is “\(term.name)”.")
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                } header: {
                    Text("Tip")
                }
                Section {
                    SessionTimer(startedAt: $timerStartedAt, screen: "exercise")
                }
            }
            .navigationTitle(exercise.title)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .safeAreaInset(edge: .bottom) {
                if let result = activity?.result {
                    Label("Done today: \(result.title)", systemImage: "checkmark.circle.fill")
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding()
                        .background(.regularMaterial)
                        .accessibilityIdentifier("exercise.done")
                } else {
                    Button {
                        recording = true
                    } label: {
                        Text("How Did It Go?")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .padding()
                    .background(.regularMaterial)
                    .accessibilityIdentifier("exercise.record")
                }
            }
            .sheet(isPresented: $recording) {
                RecordSessionView(model: model, route: route, title: exercise.title) { feedback = $0 }
            }
            #if os(iOS)
            .fullScreenCover(item: $guided) { session in
                GuidedSessionView(model: model, session: session, timerStartedAt: $timerStartedAt)
            }
            #else
            .sheet(item: $guided) { session in
                GuidedSessionView(model: model, session: session, timerStartedAt: $timerStartedAt)
            }
            #endif
            .errorAlert("Couldn't Start", message: $startError)
            .alert(
                "Session Saved",
                isPresented: Binding(get: { feedback != nil }, set: { if !$0 { feedback = nil } }),
                presenting: feedback
            ) { _ in
                Button("OK", role: .cancel) {}
                    .accessibilityIdentifier("feedback.ok")
            } message: { feedback in
                Text(feedback.message)
            }
            .sensoryFeedback(.success, trigger: model.sessionCount)
        } else {
            ContentUnavailableView("Exercise Not Found", systemImage: "questionmark.circle")
        }
    }

    /// A new session every time, on its "get ready" card. The timer starts
    /// with it unless it is already running.
    private func startGuided() {
        do {
            guided = try model.startGuidedSession(exerciseId: route.exerciseId, activityId: route.activityId)
            if timerStartedAt == nil { timerStartedAt = Date() }
        } catch {
            startError = GoodDogModel.describe(error)
        }
    }
}

extension GuidedSessionModel: Identifiable {}

/// A simple stopwatch, so sessions stay short. `screen` prefixes the
/// accessibility identifiers (`exercise.timer.start`, `guided.timer.stop`).
struct SessionTimer: View {
    @Binding var startedAt: Date?
    let screen: String

    var body: some View {
        // Side by side, or one above the other at the largest text sizes.
        ViewThatFits(in: .horizontal) {
            HStack {
                time
                Spacer(minLength: 12)
                button
            }
            VStack(alignment: .leading, spacing: 8) {
                time
                button
            }
        }
        .sensoryFeedback(.selection, trigger: startedAt)
    }

    @ViewBuilder private var time: some View {
        if let startedAt {
            TimelineView(.periodic(from: startedAt, by: 1)) { context in
                let seconds = max(0, Int(context.date.timeIntervalSince(startedAt)))
                Text(String(format: "%d:%02d", seconds / 60, seconds % 60))
                    .font(.title2.monospacedDigit())
                    .accessibilityLabel("Session time: \(seconds / 60) minutes \(seconds % 60) seconds")
                    .accessibilityIdentifier("\(screen).timer")
            }
        } else {
            Label("Timer", systemImage: "stopwatch")
                .foregroundStyle(.secondary)
        }
    }

    /// A bordered button at least 44 pt tall: the bare text button measured
    /// 59 x 20 pt on the phone (forge_ui_audit, Stage B), too small to hit
    /// while holding a lead.
    @ViewBuilder private var button: some View {
        if startedAt != nil {
            Button { startedAt = nil } label: {
                Label("Stop", systemImage: "stop.fill")
                    .frame(minWidth: 44, minHeight: 44)
            }
            .buttonStyle(.bordered)
            .accessibilityLabel("Stop timer")
            .accessibilityIdentifier("\(screen).timer.stop")
        } else {
            Button { startedAt = Date() } label: {
                Label("Start", systemImage: "play.fill")
                    .frame(minWidth: 44, minHeight: 44)
            }
            .buttonStyle(.bordered)
            .accessibilityLabel("Start timer")
            .accessibilityIdentifier("\(screen).timer.start")
        }
    }
}

/// The outcome choice: how it went, and whether the dog seemed worried or
/// uncomfortable. Shared by "How Did It Go?" and the guided session's last
/// card, so both offer exactly the same choice. Sections, for a `Form`.
struct OutcomeChoice: View {
    let title: String
    @Binding var result: SessionResult?
    @Binding var welfareConcern: Bool

    var body: some View {
        Section {
            ForEach(SessionResult.allCases) { option in
                Button {
                    result = option
                } label: {
                    HStack {
                        Label(option.title, systemImage: option.systemImage)
                            .foregroundStyle(.primary)
                        Spacer()
                        if result == option {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.tint)
                                .accessibilityHidden(true)
                        }
                    }
                }
                .accessibilityAddTraits(result == option ? .isSelected : [])
                .accessibilityIdentifier("record.result.\(option.key)")
            }
        } header: {
            Text(title)
        }
        Section {
            Toggle("My dog seemed worried, sore or uncomfortable", isOn: $welfareConcern)
                .accessibilityIdentifier("record.welfare")
        } footer: {
            Text("If so, we'll rest this skill for a few days and keep things gentle.")
        }
    }
}

/// "How did it go?": the result, and whether the dog seemed worried or uncomfortable.
private struct RecordSessionView: View {
    let model: GoodDogModel
    let route: ExerciseRoute
    let title: String
    let onSaved: (SessionFeedback) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var result: SessionResult?
    @State private var welfareConcern = false
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                OutcomeChoice(title: title, result: $result, welfareConcern: $welfareConcern)
            }
            .navigationTitle("How Did It Go?")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("record.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { Task { await save() } }
                        .disabled(result == nil || saving)
                        .accessibilityIdentifier("record.save")
                }
            }
            .sensoryFeedback(.selection, trigger: result)
            .errorAlert("Couldn't Save", message: $error)
        }
        .presentationDetents([.medium, .large])
    }

    private func save() async {
        guard let result else { return }
        saving = true
        defer { saving = false }
        do {
            let feedback = try await model.record(
                exerciseId: route.exerciseId,
                activityId: route.activityId,
                result: result,
                welfareConcern: welfareConcern
            )
            dismiss()
            onSaved(feedback)
        } catch {
            self.error = GoodDogModel.describe(error)
        }
    }
}
