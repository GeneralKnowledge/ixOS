@preconcurrency import GoodDogFFI
import ForgeUI
import SwiftUI

/// The exercise library by topic, the glossary, and search across both.
struct LearnView: View {
    @Bindable var model: GoodDogModel

    var body: some View {
        DogStack(model: model) {
            List {
                if let results = model.searchResults {
                    if !results.exercises.isEmpty {
                        Section("Exercises") {
                            ForEach(results.exercises) { exerciseLink($0) }
                        }
                    }
                    if !results.terms.isEmpty {
                        Section("Training words") {
                            ForEach(results.terms) { term in
                                NavigationLink(value: DogRoute.term(term.id)) { TermRow(term: term) }
                                    .accessibilityIdentifier("learn.term.\(term.id)")
                            }
                        }
                    }
                } else {
                    Section {
                        NavigationLink(value: DogRoute.glossary) {
                            Label("Training Words", systemImage: "character.book.closed")
                        }
                        .accessibilityIdentifier("learn.glossary")
                    } footer: {
                        Text("Plain-English meanings of the words trainers use.")
                    }
                    ForEach(model.topics) { topic in
                        Section {
                            ForEach(topic.exercises) { exerciseLink($0) }
                        } header: {
                            Text(topic.title)
                        } footer: {
                            Text(topic.description)
                        }
                    }
                }
            }
            .overlay {
                if let results = model.searchResults, results.exercises.isEmpty, results.terms.isEmpty {
                    ForgeStateView.noResults(for: model.searchText)
                }
            }
            .searchable(text: $model.searchText, prompt: "Exercises and words")
            .navigationTitle("Learn")
        }
    }

    private func exerciseLink(_ exercise: ExerciseSummary) -> some View {
        NavigationLink(value: DogRoute.exercise(ExerciseRoute(exerciseId: exercise.id, activityId: nil))) {
            ExerciseRow(exercise: exercise)
        }
        .accessibilityIdentifier("learn.exercise.\(exercise.id)")
    }
}

struct GlossaryView: View {
    let model: GoodDogModel

    var body: some View {
        List {
            ForEach(model.glossary) { section in
                Section {
                    ForEach(section.terms) { term in
                        NavigationLink(value: DogRoute.term(term.id)) { TermRow(term: term) }
                            .accessibilityIdentifier("glossary.term.\(term.id)")
                    }
                } header: {
                    Text(section.title)
                } footer: {
                    Text(section.description)
                }
            }
        }
        .navigationTitle("Training Words")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }
}
