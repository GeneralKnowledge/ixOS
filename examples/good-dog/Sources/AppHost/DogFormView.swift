@preconcurrency import ForgeFFI
import SwiftUI

/// The dog's details, for onboarding and for editing on My Dog. The form
/// only collects; Rust validates and saves.
struct DogFormFields: View {
    @Binding var name: String
    @Binding var lifeStage: DogLifeStage
    @Binding var dailyTime: DailyTime
    @Binding var goal: TrainingGoal
    @Binding var experience: OwnerExperience
    var focusName: FocusState<Bool>.Binding

    var body: some View {
        Section {
            TextField("Name", text: $name)
                .textContentType(.name)
                .focused(focusName)
                .submitLabel(.done)
                .accessibilityLabel("Dog's name")
                .accessibilityIdentifier("dogForm.name")
        } header: {
            Text("Your dog's name")
        }
        Section {
            Picker("Age", selection: $lifeStage) {
                ForEach(DogLifeStage.allCases) { Text($0.title).tag($0) }
            }
            .accessibilityIdentifier("dogForm.lifeStage")
            Picker("Time Most Days", selection: $dailyTime) {
                ForEach(DailyTime.allCases) { Text($0.title).tag($0) }
            }
            .accessibilityIdentifier("dogForm.dailyTime")
        } footer: {
            Text("Short sessions work best. We'll plan around the time you have.")
        }
        Section("What would help most?") {
            Picker("Goal", selection: $goal) {
                ForEach(TrainingGoal.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.inline)
            .labelsHidden()
            .accessibilityIdentifier("dogForm.goal")
            Picker("Experience", selection: $experience) {
                ForEach(OwnerExperience.allCases) { Text($0.title).tag($0) }
            }
            .accessibilityIdentifier("dogForm.experience")
        }
    }
}

/// First launch: tell Good Dog about the dog.
struct OnboardingView: View {
    let model: GoodDogModel
    @State private var name = ""
    @State private var lifeStage: DogLifeStage = .adult
    @State private var dailyTime: DailyTime = .aboutTenMinutes
    @State private var goal: TrainingGoal = .general
    @State private var experience: OwnerExperience = .new
    @State private var error: String?
    @State private var saving = false
    @FocusState private var nameFocused: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Short, kind, reward-based practice, planned for you each day.")
                        .foregroundStyle(.secondary)
                }
                DogFormFields(
                    name: $name, lifeStage: $lifeStage, dailyTime: $dailyTime,
                    goal: $goal, experience: $experience, focusName: $nameFocused
                )
                Section {
                    Button {
                        Task { await start() }
                    } label: {
                        Text("Start Training")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || saving)
                    .accessibilityIdentifier("onboarding.start")
                } footer: {
                    Text("Good Dog gives general training guidance. It isn't veterinary or behavioural advice.")
                }
            }
            .scrollDismissesKeyboard(.interactively)
            .navigationTitle("Welcome to Good Dog")
            .errorAlert("Couldn't Save", message: $error)
        }
    }

    private func start() async {
        saving = true
        defer { saving = false }
        do {
            try await model.saveDog(DogProfile(
                name: name, lifeStage: lifeStage, dailyTime: dailyTime,
                goal: goal, experience: experience
            ))
        } catch {
            self.error = GoodDogModel.describe(error)
        }
    }
}

/// My Dog's edit sheet.
struct EditDogView: View {
    let model: GoodDogModel
    let dog: DogProfile
    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var lifeStage: DogLifeStage
    @State private var dailyTime: DailyTime
    @State private var goal: TrainingGoal
    @State private var experience: OwnerExperience
    @State private var error: String?
    @FocusState private var nameFocused: Bool

    init(model: GoodDogModel, dog: DogProfile) {
        self.model = model
        self.dog = dog
        _name = State(initialValue: dog.name)
        _lifeStage = State(initialValue: dog.lifeStage)
        _dailyTime = State(initialValue: dog.dailyTime)
        _goal = State(initialValue: dog.goal)
        _experience = State(initialValue: dog.experience)
    }

    private var edited: DogProfile {
        DogProfile(name: name, lifeStage: lifeStage, dailyTime: dailyTime, goal: goal, experience: experience)
    }

    var body: some View {
        NavigationStack {
            Form {
                DogFormFields(
                    name: $name, lifeStage: $lifeStage, dailyTime: $dailyTime,
                    goal: $goal, experience: $experience, focusName: $nameFocused
                )
            }
            .scrollDismissesKeyboard(.interactively)
            .navigationTitle("Edit Details")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("dogForm.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        Task {
                            do {
                                try await model.saveDog(edited)
                                dismiss()
                            } catch {
                                self.error = GoodDogModel.describe(error)
                            }
                        }
                    }
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityIdentifier("dogForm.save")
                }
            }
            .interactiveDismissDisabled(edited != dog)
            .errorAlert("Couldn't Save", message: $error)
        }
    }
}
