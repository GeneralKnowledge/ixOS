@preconcurrency import GoodDogFFI
import SwiftUI

/// Text from Rust whose glossary terms are tappable. A tap opens the term in
/// a sheet (`termSheet`), so owners can learn the proper word without
/// leaving the exercise.
struct LinkedTextView: View {
    let text: LinkedText

    var body: some View {
        Text(attributed)
    }

    private var attributed: AttributedString {
        var result = AttributedString()
        for piece in text.pieces {
            var run = AttributedString(piece.text)
            if let id = piece.termId, let url = URL(string: "gooddog-term:\(id)") {
                run.link = url
                run.underlineStyle = .single
            }
            result += run
        }
        return result
    }
}

private struct TermSheet: ViewModifier {
    let model: GoodDogModel
    @State private var termId: TermID?

    struct TermID: Identifiable {
        let id: String
    }

    func body(content: Content) -> some View {
        content
            .environment(\.openURL, OpenURLAction { url in
                guard url.scheme == "gooddog-term" else { return .systemAction }
                termId = TermID(id: url.absoluteString.replacingOccurrences(of: "gooddog-term:", with: ""))
                return .handled
            })
            .sheet(item: $termId) { term in
                NavigationStack {
                    TermView(model: model, termId: term.id)
                        .toolbar {
                            ToolbarItem(placement: .confirmationAction) {
                                Button("Done") { termId = nil }
                                    .accessibilityIdentifier("term.done")
                            }
                        }
                }
                .presentationDetents([.medium, .large])
            }
    }
}

extension View {
    /// Opens tapped glossary links (from `LinkedTextView`) in a sheet. Attach
    /// once, above every view that shows linked text.
    func termSheet(model: GoodDogModel) -> some View {
        modifier(TermSheet(model: model))
    }
}

/// One glossary term in full.
struct TermView: View {
    let model: GoodDogModel
    let termId: String

    var body: some View {
        if let term = model.term(id: termId) {
            List {
                Section {
                    Text(term.shortDefinition)
                        .font(.headline)
                    if !term.alsoCalled.isEmpty {
                        LabeledContent("Also called", value: term.alsoCalled.joined(separator: ", "))
                    }
                }
                Section("In more detail") {
                    LinkedTextView(text: term.explanation)
                }
                Section("For example") {
                    LinkedTextView(text: term.example)
                }
                if let misunderstanding = term.commonMisunderstanding {
                    Section("A common mix-up") {
                        LinkedTextView(text: misunderstanding)
                    }
                }
                if !term.related.isEmpty {
                    Section("Related words") {
                        ForEach(term.related) { related in
                            NavigationLink(value: TermRoute(id: related.id)) {
                                TermRow(term: related)
                            }
                            .accessibilityIdentifier("term.related.\(related.id)")
                        }
                    }
                }
                if !term.exercises.isEmpty {
                    Section("Practise it") {
                        ForEach(term.exercises) { exercise in
                            NavigationLink(value: ExerciseRoute(exerciseId: exercise.id, activityId: nil)) {
                                ExerciseRow(exercise: exercise)
                            }
                            .accessibilityIdentifier("term.exercise.\(exercise.id)")
                        }
                    }
                }
            }
            .navigationTitle(term.name)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .navigationDestination(for: TermRoute.self) { TermView(model: model, termId: $0.id) }
            .navigationDestination(for: ExerciseRoute.self) { ExerciseView(model: model, route: $0) }
        } else {
            ContentUnavailableView("Term Not Found", systemImage: "questionmark.circle")
        }
    }
}

struct TermRoute: Hashable {
    let id: String
}

struct TermRow: View {
    let term: TermSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(term.name).font(.headline)
            Text(term.shortDefinition)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
    }
}

struct ExerciseRow: View {
    let exercise: ExerciseSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(exercise.title).font(.headline)
            Text("\(exercise.minutes) min · \(exercise.summary)")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
    }
}
