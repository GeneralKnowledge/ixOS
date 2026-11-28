@preconcurrency import GoodDogFFI
import ForgeUI
import ForgeAppKit
import SwiftUI

/// The dog's details, progress in plain English, recent sessions, and erasing everything.
struct MyDogView: View {
    @Bindable var model: GoodDogModel
    @State private var editing = false
    @State private var confirmErase = false
    @State private var choosingGoal = false

    /// 18:00 unless the owner picks another time.
    private static let defaultReminderMinute: UInt32 = 18 * 60

    private var reminderOn: Binding<Bool> {
        Binding(
            get: { model.reminderMinute != nil },
            set: { on in
                Task { await model.setReminder(on ? Self.defaultReminderMinute : nil) }
            }
        )
    }

    /// The reminder's minute after midnight as a time today, for the picker.
    private var reminderTime: Binding<Date> {
        let calendar = Calendar.current
        return Binding(
            get: {
                let minute = Int(model.reminderMinute ?? Self.defaultReminderMinute)
                return calendar.date(
                    bySettingHour: minute / 60, minute: minute % 60, second: 0, of: .now
                ) ?? .now
            },
            set: { date in
                let parts = calendar.dateComponents([.hour, .minute], from: date)
                let minute = UInt32((parts.hour ?? 18) * 60 + (parts.minute ?? 0))
                Task { await model.setReminder(minute) }
            }
        )
    }

    var body: some View {
        DogStack(model: model) {
            List {
                if let dog = model.dog {
                    Section {
                        ForgeDetailHeader(dog.name, subtitle: dog.lifeStage.title, systemImage: "pawprint.fill") {
                            ForgeBadge(dog.goal.title, systemImage: "scope", tone: .accent)
                        }
                        .listRowBackground(Color.clear)
                        .accessibilityIdentifier("dog.header")
                    }
                    Section {
                        LabeledContent("Time most days", value: dog.dailyTime.title)
                        LabeledContent("Experience", value: dog.experience.title)
                    }
                }
                if model.dog != nil, !model.pathways.isEmpty {
                    Section {
                        ForEach(model.goals) { goal in
                            NavigationLink(value: DogRoute.goal(goal.id)) {
                                ForgeRow(
                                    goal.title,
                                    subtitle: goal.status == .finished
                                        ? goal.status.title
                                        : String(localized: "Stage \(goal.stageNumber) of \(goal.stageCount): \(goal.stageTitle)")
                                ) {
                                    if goal.status == .paused || goal.status == .setAside {
                                        ForgeBadge(goal.status.title, tone: .warning)
                                    }
                                }
                            }
                            .accessibilityIdentifier("dog.goal.\(goal.pathwayId)")
                        }
                        Button("Add a Goal", systemImage: "plus") { choosingGoal = true }
                            .accessibilityIdentifier("dog.addGoal")
                    } header: {
                        Text("Goals")
                    } footer: {
                        Text("Work on up to three at once. Pausing or setting one aside keeps its progress.")
                    }
                }
                if let progress = model.progress {
                    Section("This week") {
                        LabeledContent("Sessions", value: "\(progress.sessionsThisWeek)")
                            .accessibilityIdentifier("dog.sessionsThisWeek")
                        LabeledContent("Days practised", value: "\(progress.daysPractisedThisWeek) of 7")
                        LabeledContent("Sessions in total", value: "\(progress.sessionsTotal)")
                    }
                    Section {
                        if progress.skills.isEmpty {
                            Text("Skills appear here once you've practised them.")
                                .foregroundStyle(.secondary)
                        }
                        ForEach(progress.skills) { skill in
                            VStack(alignment: .leading, spacing: 2) {
                                HStack {
                                    Text(skill.name).font(.headline)
                                    Spacer()
                                    Text(skill.level.title)
                                        .font(.subheadline)
                                        .forgeForeground(skill.level == .makingEasier ? .warning : .neutral)
                                }
                                if !skill.description.isEmpty {
                                    Text(skill.description)
                                        .font(.subheadline)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            .accessibilityElement(children: .combine)
                            .accessibilityIdentifier("dog.skill.\(skill.objectiveId)")
                        }
                    } header: {
                        Text("Skills")
                    }
                }
                if !model.history.isEmpty {
                    Section("Recent sessions") {
                        ForEach(model.history.prefix(20)) { session in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(session.exerciseTitle).font(.headline)
                                HStack {
                                    Label(session.result.title, systemImage: session.result.systemImage)
                                    if session.welfareConcern {
                                        Label("Seemed uncomfortable", systemImage: "heart")
                                            .forgeForeground(.warning)
                                    }
                                }
                                .font(.subheadline)
                                Text(session.completedAt, format: .dateTime.weekday(.wide).hour().minute())
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                            }
                            .accessibilityElement(children: .combine)
                        }
                    }
                }
                if model.dog != nil {
                    Section {
                        Toggle("Daily Reminder", isOn: reminderOn)
                            .accessibilityIdentifier("dog.reminders.toggle")
                        if model.reminderMinute != nil {
                            DatePicker("Time", selection: reminderTime, displayedComponents: .hourAndMinute)
                                .accessibilityIdentifier("dog.reminders.time")
                        }
                    } header: {
                        Text("Reminder")
                    } footer: {
                        Text("One nudge a day, skipped once you've practised. Tap Not Today on it to skip a day.")
                    }
                }
                Section {
                    Button("Erase All Data", role: .destructive) { confirmErase = true }
                        .accessibilityIdentifier("dog.erase")
                } footer: {
                    Text("Good Dog keeps everything on this iPhone. It gives general training guidance, not veterinary or behavioural advice.")
                }
            }
            .navigationTitle("My Dog")
            .sheet(isPresented: $choosingGoal) { GoalPickerView(model: model) }
            .toolbar {
                if model.dog != nil {
                    ToolbarItem(placement: .primaryAction) {
                        Button("Edit") { editing = true }
                            .accessibilityIdentifier("dog.edit")
                    }
                }
            }
            .sheet(isPresented: $editing) {
                if let dog = model.dog {
                    EditDogView(model: model, dog: dog)
                }
            }
            .confirmationDialog("Erase everything?", isPresented: $confirmErase, titleVisibility: .visible) {
                Button("Erase All Data", role: .destructive) { Task { await model.eraseAll() } }
                    .accessibilityIdentifier("dog.erase.confirm")
            } message: {
                Text("This deletes your dog's details, plans, sessions and progress. It can't be undone.")
            }
        }
    }
}
