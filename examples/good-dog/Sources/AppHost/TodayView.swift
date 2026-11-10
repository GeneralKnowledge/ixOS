@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// Today's plan: two or three short activities, each with why it was chosen.
struct TodayView: View {
    @Bindable var model: GoodDogModel

    var body: some View {
        NavigationStack {
            List {
                if let plan = model.today {
                    Section {
                        ProgressRow(plan: plan, dogName: model.dog?.name ?? "your dog")
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
                    if plan.canShorten {
                        Section {
                            Button("Only a Few Minutes Today", systemImage: "timer") {
                                Task { await model.shortenToday() }
                            }
                            .accessibilityIdentifier("today.shorten")
                        }
                    }
                }
            }
            .overlay {
                if model.today?.activities.isEmpty == true {
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
            .errorAlert("Something Went Wrong", message: $model.actionError)
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
