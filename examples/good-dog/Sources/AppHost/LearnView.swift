@preconcurrency import ForgeFFI
import SwiftUI

/// The exercise library by topic, the glossary, and search across both.
struct LearnView: View {
    @Bindable var model: GoodDogModel

    var body: some View {
        NavigationStack {
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
                                NavigationLink(value: TermRoute(id: term.id)) { TermRow(term: term) }
                                    .accessibilityIdentifier("learn.term.\(term.id)")
                            }
                        }
                    }
                } else {
                    Section {
                        NavigationLink(value: GlossaryRoute()) {
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
                    ContentUnavailableView.search(text: model.searchText)
                }
            }
            .searchable(text: $model.searchText, prompt: "Exercises and words")
            .navigationTitle("Learn")
            .navigationDestination(for: ExerciseRoute.self) { ExerciseView(model: model, route: $0) }
            .navigationDestination(for: TermRoute.self) { TermView(model: model, termId: $0.id) }
            .navigationDestination(for: GlossaryRoute.self) { _ in GlossaryView(model: model) }
        }
    }

    private func exerciseLink(_ exercise: ExerciseSummary) -> some View {
        NavigationLink(value: ExerciseRoute(exerciseId: exercise.id, activityId: nil)) {
            ExerciseRow(exercise: exercise)
        }
        .accessibilityIdentifier("learn.exercise.\(exercise.id)")
    }
}

struct GlossaryRoute: Hashable {}

struct GlossaryView: View {
    let model: GoodDogModel

    var body: some View {
        List {
            ForEach(model.glossary) { section in
                Section {
                    ForEach(section.terms) { term in
                        NavigationLink(value: TermRoute(id: term.id)) { TermRow(term: term) }
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
