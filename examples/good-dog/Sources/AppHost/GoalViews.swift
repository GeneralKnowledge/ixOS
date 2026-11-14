@preconcurrency import GoodDogFFI
import ForgeUI
import SwiftUI

/// Which goal to show.
struct GoalRoute: Hashable {
    let goalId: UInt64
}

// MARK: The goal's path

/// One goal as a path of stages, like a level map: badges for passed stages,
/// the current stage lit up, later ones waiting. Below it, what this stage
/// means, why it matters, how to practise, and the question that moves on.
struct GoalPathView: View {
    @Bindable var model: GoodDogModel
    let goalId: UInt64
    @State private var detail: GoalDetail?
    @State private var confirmSetAside = false

    var body: some View {
        Group {
            if let detail {
                content(detail)
            } else {
                ForgeStateView.empty(
                    "Goal Not Found",
                    systemImage: "map",
                    description: "This goal is no longer here."
                )
            }
        }
        .navigationTitle(detail?.summary.title ?? "Goal")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .task(id: model.revision) { detail = model.goal(id: goalId) }
        .toolbar {
            if let summary = detail?.summary {
                ToolbarItem(placement: .primaryAction) {
                    Menu("Options", systemImage: "ellipsis.circle") {
                        switch summary.status {
                        case .active:
                            Button("Pause for Now", systemImage: "pause.circle") {
                                Task { await model.pauseGoal(goalId) }
                            }
                            .accessibilityIdentifier("goal.pause")
                        case .paused, .setAside:
                            Button("Pick Up Again", systemImage: "play.circle") {
                                Task { await model.resumeGoal(goalId) }
                            }
                            .accessibilityIdentifier("goal.resume")
                        case .finished:
                            EmptyView()
                        }
                        if summary.status == .active, summary.stageNumber > 1 {
                            Button("Go Back a Stage", systemImage: "arrow.uturn.backward") {
                                Task { await model.stepBack(goalId) }
                            }
                            .accessibilityIdentifier("goal.stepBack")
                        }
                        if summary.status != .setAside, summary.status != .finished {
                            Button("Set Aside", systemImage: "archivebox") { confirmSetAside = true }
                                .accessibilityIdentifier("goal.setAside")
                        }
                    }
                    .accessibilityIdentifier("goal.options")
                }
            }
        }
        .confirmationDialog("Set this goal aside?", isPresented: $confirmSetAside, titleVisibility: .visible) {
            Button("Set Aside") { Task { await model.setGoalAside(goalId) } }
                .accessibilityIdentifier("goal.setAside.confirm")
        } message: {
            Text("Its stages and history are kept. You can pick it up again from My Dog.")
        }
        .errorAlert(LocalizedStringKey(model.actionErrorTitle), message: $model.actionError)
    }

    @ViewBuilder
    private func content(_ detail: GoalDetail) -> some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    ForgeBadge(
                        detail.summary.status.title,
                        systemImage: detail.summary.status == .finished ? "trophy" : nil,
                        tone: detail.summary.status == .active ? .accent : .neutral
                    )
                    Text(detail.pathwaySummary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    if let words = detail.summary.ownWords {
                        Text("You said: \u{201c}\(words)\u{201d}")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
                .accessibilityElement(children: .combine)
                StagePath(stages: detail.stages, finished: detail.summary.status == .finished)
                    .listRowInsets(EdgeInsets(top: 12, leading: 8, bottom: 12, trailing: 8))
            }
            if let current = detail.stages.first(where: { $0.state == .current }) {
                Section {
                    Text(current.whatItMeans)
                    LabeledText("Why it matters", current.whyItMatters)
                    LabeledText("In everyday life", current.everydayPractice, systemImage: "house")
                    LabeledText("Ready when", current.readyWhen, systemImage: "flag.checkered")
                } header: {
                    Text("Now: \(current.title)")
                }
                Section("Practise") {
                    ForEach(current.exercises) { exercise in
                        NavigationLink(value: ExerciseRoute(exerciseId: exercise.id, activityId: nil)) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(exercise.title).font(.headline)
                                Text(exercise.summary)
                                    .font(.subheadline)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .accessibilityIdentifier("goal.exercise.\(exercise.id)")
                    }
                }
                if let gate = detail.gate, detail.summary.status == .active {
                    GateSection(model: model, goalId: goalId, gate: gate, ifItIsHard: current.ifItIsHard)
                }
            } else if detail.summary.status == .finished,
                      let last = detail.stages.last {
                Section("Keeping it up") {
                    Text(last.maintenance)
                }
            }
            if !detail.history.isEmpty {
                Section("Your story so far") {
                    ForEach(Array(detail.history.prefix(10).enumerated()), id: \.offset) { _, event in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(event.text)
                            Text(event.at, format: .dateTime.day().month().hour().minute())
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }
        }
    }
}

/// A label above a paragraph.
private struct LabeledText: View {
    let title: String
    let text: String
    let systemImage: String?

    init(_ title: String, _ text: String, systemImage: String? = nil) {
        self.title = title
        self.text = text
        self.systemImage = systemImage
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let systemImage {
                Label(title, systemImage: systemImage).font(.subheadline.weight(.semibold))
            } else {
                Text(title).font(.subheadline.weight(.semibold))
            }
            Text(text)
        }
        .accessibilityElement(children: .combine)
    }
}

/// The stages as a winding path of round badges.
private struct StagePath: View {
    let stages: [GoalStage]
    let finished: Bool
    @Environment(\.forgeTheme) private var theme
    @Environment(\.dynamicTypeSize) private var typeSize

    /// Left, centre, right, centre... so the path winds.
    private func offset(_ index: Int) -> CGFloat {
        guard !typeSize.isAccessibilitySize else { return 0 }
        return [0, 1, 0, -1][index % 4] * 56
    }

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(stages.enumerated()), id: \.element.id) { index, stage in
                if index > 0 {
                    Capsule()
                        .fill(stage.state == .upcoming ? theme.colors.separator : theme.colors.tone(.success).solid)
                        .frame(width: 6, height: 22)
                        .offset(x: (offset(index - 1) + offset(index)) / 2)
                        .accessibilityHidden(true)
                }
                StageNode(stage: stage, number: index + 1, count: stages.count)
                    .offset(x: offset(index))
            }
            if finished {
                Image(systemName: "trophy.fill")
                    .font(.largeTitle)
                    .forgeForeground(.warning)
                    .padding(.top, 10)
                    .accessibilityLabel("Goal reached")
            }
        }
        .frame(maxWidth: .infinity)
    }
}

private struct StageNode: View {
    let stage: GoalStage
    let number: Int
    let count: Int
    @Environment(\.forgeTheme) private var theme
    @State private var glow = false

    private var symbol: String {
        switch stage.state {
        case .passed: "checkmark"
        case .current: "pawprint.fill"
        case .upcoming: "lock.fill"
        }
    }

    private var fill: Color {
        switch stage.state {
        case .passed: theme.colors.tone(.success).solid
        case .current: theme.colors.accent.solid
        case .upcoming: theme.colors.separator
        }
    }

    var body: some View {
        VStack(spacing: 6) {
            ZStack {
                if stage.state == .current {
                    Circle()
                        .stroke(theme.colors.accent.solid.opacity(0.35), lineWidth: 6)
                        .frame(width: 84, height: 84)
                        .scaleEffect(glow ? 1.08 : 0.96)
                        .animation(.easeInOut(duration: 1.4).repeatForever(autoreverses: true), value: glow)
                }
                Circle()
                    .fill(fill)
                    .frame(width: 68, height: 68)
                    .shadow(color: .black.opacity(stage.state == .upcoming ? 0 : 0.15), radius: 3, y: 3)
                Image(systemName: symbol)
                    .font(.title2.weight(.bold))
                    .foregroundStyle(stage.state == .upcoming ? theme.colors.textTertiary : .white)
            }
            Text(stage.title)
                .font(.subheadline.weight(stage.state == .current ? .semibold : .regular))
                .foregroundStyle(stage.state == .upcoming ? .secondary : .primary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 200)
        }
        .onAppear { glow = true }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Stage \(number) of \(count): \(stage.title)")
        .accessibilityValue(stage.state == .passed ? "Passed" : stage.state == .current ? "Working on it now" : "Still to come")
        .accessibilityIdentifier("goal.stage.\(number - 1)")
    }
}

/// The question that moves a goal on, with the app's note on where things are.
private struct GateSection: View {
    let model: GoodDogModel
    let goalId: UInt64
    let gate: GateStatus
    let ifItIsHard: String
    @State private var showHard = false

    var body: some View {
        Section {
            Text(gate.note)
                .font(.subheadline)
                .forgeForeground(gate.held ? .warning : .neutral)
                .accessibilityIdentifier("goal.gate.note")
            if !gate.held {
                Text(gate.question)
                    .font(.headline)
                    .accessibilityIdentifier("goal.gate.question")
                HStack {
                    Button {
                        Task { await model.answerGate(goalId, met: true) }
                    } label: {
                        Text("Yes").frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .accessibilityLabel("Yes, we're there")
                    .accessibilityIdentifier("goal.gate.yes")
                    Button {
                        showHard = true
                        Task { await model.answerGate(goalId, met: false) }
                    } label: {
                        Text("Not Yet").frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)
                    .accessibilityIdentifier("goal.gate.notYet")
                }
            }
            if showHard || gate.held {
                LabeledText("If it's hard", ifItIsHard, systemImage: "tortoise")
            }
        } header: {
            Text("Ready to move on?")
        } footer: {
            Text("Moving on is your call: you see your dog every day. A stage that's harder than expected can always go back.")
        }
    }
}

// MARK: Choosing a goal

/// Pick something to work on, in your own words or from the list.
struct GoalPickerView: View {
    let model: GoodDogModel
    @Environment(\.dismiss) private var dismiss
    @State private var ownWords = ""
    @State private var error: String?
    @State private var starting = false

    private var suggestions: [PathwaySummary] { model.suggestGoals(ownWords) }
    private var started: Set<String> { Set(model.goals.filter { $0.status != .setAside }.map(\.pathwayId)) }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    TextField("For example: she pulls on the lead", text: $ownWords, axis: .vertical)
                        .lineLimit(1...3)
                        .accessibilityLabel("What would you like to change, in your own words")
                        .accessibilityIdentifier("goalPicker.words")
                } header: {
                    Text("In your own words")
                } footer: {
                    if !ownWords.isEmpty && suggestions.isEmpty {
                        Text("Nothing here matches that yet. Pick the closest from the list.")
                    }
                }
                if !suggestions.isEmpty {
                    Section("Sounds like") {
                        ForEach(suggestions) { row($0) }
                    }
                }
                Section("All goals") {
                    ForEach(model.pathways) { row($0) }
                }
            }
            .navigationTitle("What to Work On")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("goalPicker.cancel")
                }
            }
            .disabled(starting)
            .errorAlert("Can't Start That Goal", message: $error)
        }
    }

    private func row(_ pathway: PathwaySummary) -> some View {
        Button {
            Task { await start(pathway) }
        } label: {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: started.contains(pathway.id) ? "checkmark.circle.fill" : "map")
                    .font(.title2)
                    .forgeForeground(started.contains(pathway.id) ? .success : .accent)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(pathway.title).font(.headline)
                    Text(pathway.summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Text("\(pathway.stageCount) stages")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .tint(.primary)
        .accessibilityValue(started.contains(pathway.id) ? "Already one of your goals" : "")
        .accessibilityIdentifier("goalPicker.pathway.\(pathway.id)")
    }

    private func start(_ pathway: PathwaySummary) async {
        starting = true
        defer { starting = false }
        do {
            try await model.startGoal(pathwayId: pathway.id, ownWords: ownWords)
            dismiss()
        } catch {
            self.error = GoodDogModel.describe(error)
        }
    }
}

// MARK: The week

/// This week's practice days against a gentle target. Rest days never
/// break anything: the week counts, not the day.
struct RhythmRow: View {
    let rhythm: WeeklyRhythm
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                ForEach(0..<7, id: \.self) { index in
                    let practised = rhythm.days.indices.contains(index) && rhythm.days[index]
                    let isToday = index == Int(rhythm.todayIndex)
                    VStack(spacing: 4) {
                        ZStack {
                            Circle()
                                .fill(practised ? theme.colors.accent.solid : theme.colors.accent.soft)
                            if practised {
                                Image(systemName: "pawprint.fill")
                                    .font(.caption.weight(.bold))
                                    .foregroundStyle(theme.colors.accent.onSolid)
                            }
                        }
                        .overlay {
                            if isToday {
                                Circle().stroke(theme.colors.accent.solid, lineWidth: 2).padding(-3)
                            }
                        }
                        .frame(width: 28, height: 28)
                        Text(WeeklyRhythm.dayInitials[index])
                            .font(.caption2.weight(isToday ? .bold : .regular))
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            .accessibilityHidden(true)
            Label(rhythm.summary, systemImage: rhythm.reachedTarget ? "star.fill" : "calendar")
                .font(.subheadline)
                .forgeForeground(rhythm.reachedTarget ? .success : .neutral)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("This week")
        .accessibilityValue("\(rhythm.daysPractised) of \(rhythm.targetDays) practice days. \(rhythm.summary)")
        .accessibilityIdentifier("today.rhythm")
    }
}

// MARK: Celebration

/// Shown when a stage is passed.
struct CelebrationView: View {
    let celebration: StageCelebration
    let done: () -> Void
    @State private var appeared = false

    var body: some View {
        VStack(spacing: 20) {
            Spacer()
            Image(systemName: celebration.nextStage == nil ? "trophy.fill" : "star.circle.fill")
                .font(.system(size: 96))
                .forgeForeground(.warning)
                .scaleEffect(appeared ? 1 : 0.4)
                .rotationEffect(.degrees(appeared ? 0 : -20))
                .animation(.spring(response: 0.5, dampingFraction: 0.55), value: appeared)
                .accessibilityHidden(true)
            Text(celebration.nextStage == nil ? "Goal reached!" : "Stage \(celebration.stageNumber) of \(celebration.stageCount) passed!")
                .font(.largeTitle.bold())
                .multilineTextAlignment(.center)
            Text(celebration.passedStage)
                .font(.title3)
                .foregroundStyle(.secondary)
            if let next = celebration.nextStage {
                Text("Next up: \(next)")
                    .font(.headline)
            } else {
                Text("\(celebration.goalTitle): done. A little practice now and then keeps it.")
                    .multilineTextAlignment(.center)
            }
            Spacer()
            Button {
                done()
            } label: {
                Text("Keep Going").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .accessibilityIdentifier("celebration.done")
        }
        .padding(24)
        .sensoryFeedback(.success, trigger: appeared)
        .onAppear { appeared = true }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("celebration")
    }
}

extension View {
    /// Presents the stage celebration whenever the model has one.
    func stageCelebration(model: GoodDogModel) -> some View {
        sheet(item: Binding(
            get: { model.celebration.map(CelebrationItem.init) },
            set: { if $0 == nil { model.celebration = nil } }
        )) { item in
            CelebrationView(celebration: item.celebration) { model.celebration = nil }
                .presentationDetents([.medium, .large])
        }
    }
}

private struct CelebrationItem: Identifiable {
    let celebration: StageCelebration
    var id: String { "\(celebration.goalTitle).\(celebration.stageNumber)" }
}
