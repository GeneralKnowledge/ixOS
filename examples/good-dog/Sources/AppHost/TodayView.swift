@preconcurrency import GoodDogFFI
import ForgeUI
import ForgeAppKit
import SwiftUI

/// Today's plan: two or three short activities, each with why it was chosen.
struct TodayView: View {
    @Bindable var model: GoodDogModel
    @State private var choosingGoal = false

    var body: some View {
        NavigationStack {
            List {
                if let plan = model.today {
                    Section {
                        ProgressRow(plan: plan, dogName: model.dog?.name ?? "your dog")
                        if let rhythm = model.progress?.rhythm {
                            RhythmRow(rhythm: rhythm)
                        }
                    }
                    if plan.notToday {
                        Section {
                            RestDayRow(dogName: model.dog?.name ?? "your dog")
                            Button("Practise After All", systemImage: "figure.walk") {
                                Task { await model.setNotToday(false) }
                            }
                            .accessibilityIdentifier("today.practiseAfterAll")
                        }
                    }
                    Section {
                        if let focus = plan.goalFocus {
                            NavigationLink(value: GoalRoute(goalId: focus.goalId)) {
                                GoalFocusRow(focus: focus)
                            }
                            .accessibilityIdentifier("today.goal")
                        } else if model.activeGoals.isEmpty, !model.pathways.isEmpty {
                            Button {
                                choosingGoal = true
                            } label: {
                                Label("Choose What to Work On", systemImage: "map")
                            }
                            .accessibilityIdentifier("today.chooseGoal")
                        }
                    } header: {
                        if plan.goalFocus != nil { Text("Your goal") }
                    }
                    Section {
                        ForEach(Array(plan.activities.enumerated()), id: \.element.id) { index, activity in
                            NavigationLink(value: ExerciseRoute(exerciseId: activity.exerciseId, activityId: activity.id)) {
                                ActivityRow(activity: activity)
                            }
                            .accessibilityIdentifier("today.activity.\(index)")
                            .swipeActions(edge: .trailing) {
                                if activity.canSwap {
                                    Button("Try Something Else", systemImage: "arrow.triangle.2.circlepath") {
                                        Task { await model.swap(activity) }
                                    }
                                    .forgeTint(.accent)
                                }
                            }
                            .contextMenu {
                                if activity.canSwap {
                                    Button("Try Something Else", systemImage: "arrow.triangle.2.circlepath") {
                                        Task { await model.swap(activity) }
                                    }
                                }
                            }
                        }
                    } header: {
                        Text(plan.shortPlan ? "Today's short session" : "Today's plan")
                    } footer: {
                        if plan.activities.contains(where: \.canSwap) {
                            Text("Swipe left on an activity to try something else.")
                        }
                    }
                    if (plan.canShorten && !plan.notToday) || (plan.canSayNotToday && !plan.notToday) {
                        Section {
                            if plan.canShorten {
                                Button("Only a Few Minutes Today", systemImage: "timer") {
                                    Task { await model.shortenToday() }
                                }
                                .accessibilityIdentifier("today.shorten")
                            }
                            if plan.canSayNotToday {
                                Button("Not Today", systemImage: "moon.zzz") {
                                    Task { await model.setNotToday(true) }
                                }
                                .accessibilityIdentifier("today.notToday")
                            }
                        } footer: {
                            if plan.canSayNotToday {
                                Text("Rest is part of learning. A day off never undoes your week.")
                            }
                        }
                    }
                }
            }
            .overlay {
                if model.today == nil, let problem = model.todayError {
                    ForgeStateView.failed(
                        "Can't Plan Today",
                        message: problem,
                        retry: ForgeStateAction("Try Again", identifier: "today.retry") {
                            Task { await model.reload() }
                        }
                    )
                } else if model.today?.activities.isEmpty == true {
                    ForgeStateView.empty(
                        "Nothing Planned",
                        systemImage: "pawprint",
                        description: "There's nothing that suits today. Have a look in Learn."
                    )
                }
            }
            .navigationTitle("Today")
            .navigationDestination(for: ExerciseRoute.self) { route in
                ExerciseView(model: model, route: route)
            }
            .navigationDestination(for: GoalRoute.self) { route in
                GoalPathView(model: model, goalId: route.goalId)
            }
            .sheet(isPresented: $choosingGoal) { GoalPickerView(model: model) }
            .forgeAlert(failure: $model.actionError)
        }
    }
}

/// Which exercise to show, and the plan activity it belongs to, if any.
struct ExerciseRoute: Hashable {
    let exerciseId: String
    let activityId: UInt64?
}

private struct ProgressRow: View {
    let plan: TodayPlan
    let dogName: String

    private var allDone: Bool { plan.doneCount == plan.activities.count && !plan.activities.isEmpty }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(allDone ? "All done for today" : "\(plan.totalMinutes) minutes with \(dogName)")
                .font(.headline)
            ProgressView(value: Double(plan.doneCount), total: Double(max(plan.activities.count, 1)))
                .accessibilityLabel("Activities done")
                .accessibilityValue("\(plan.doneCount) of \(plan.activities.count)")
            if allDone {
                Text("Lovely work. Come back tomorrow for a new plan.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else if plan.starter {
                Text("A gentle start while you both get used to training.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("today.progress")
    }
}

private struct ActivityRow: View {
    let activity: PlanActivity
    @Environment(\.dynamicTypeSize) private var typeSize

    /// At accessibility text sizes the status icon goes above the text, so
    /// the title keeps the full width instead of breaking mid-word.
    private var layout: AnyLayout {
        typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 6))
            : AnyLayout(HStackLayout(alignment: .top, spacing: 12))
    }

    var body: some View {
        layout {
            Image(systemName: activity.result == nil ? "circle" : "checkmark.circle.fill")
                .forgeForeground(activity.result == nil ? .neutral : .success)
                .font(.title3)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text(activity.title)
                    .font(.headline)
                Text("\(activity.role.title) · \(activity.minutes) min")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Text(activity.whyToday)
                    .font(.subheadline)
                if let result = activity.result {
                    Label(result.title, systemImage: result.systemImage)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .accessibilityValue(activity.result == nil ? "Not done yet" : "Done")
    }
}

/// The goal today's plan works towards: where the dog is on its path, and
/// one thing to try in ordinary life.
private struct GoalFocusRow: View {
    let focus: GoalFocus

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(focus.goalTitle).font(.headline)
                Spacer()
                ForgeBadge("\(focus.stageNumber) of \(focus.stageCount)", systemImage: "flag", tone: .accent)
            }
            ProgressView(value: Double(focus.stageNumber - 1), total: Double(max(focus.stageCount, 1)))
                .forgeTint(.accent)
                .accessibilityHidden(true)
            Text("Now: \(focus.stageTitle)")
                .font(.subheadline.weight(.semibold))
            Label(focus.everydayPractice, systemImage: "house")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            if let lesson = focus.nextLesson {
                Label("Next lesson: \(lesson.title)", systemImage: lesson.kind.systemImage)
                    .font(.subheadline.weight(.semibold))
                    .forgeForeground(.accent)
            } else if focus.lessonCount > 0 {
                Label("Every lesson in this stage done", systemImage: "checkmark.seal.fill")
                    .font(.subheadline)
                    .forgeForeground(.success)
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Your goal: \(focus.goalTitle). Stage \(focus.stageNumber) of \(focus.stageCount), \(focus.stageTitle). Lessons: \(focus.lessonsDone) of \(focus.lessonCount) done")
        .accessibilityHint("Shows the whole path")
    }
}

/// "Not today" was chosen.
private struct RestDayRow: View {
    let dogName: String

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "moon.zzz.fill")
                .font(.title2)
                .forgeForeground(.info)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text("A rest day for \(dogName)").font(.headline)
                Text("No reminder today. Rest helps learning settle in.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("today.restDay")
    }
}
