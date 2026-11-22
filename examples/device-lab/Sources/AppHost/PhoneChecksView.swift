import ForgeKit
import ForgeUI
import SwiftUI

/// The next phone sitting's checks, ticked on the phone and shared as text.
struct PhoneChecksView: View {
    @State private var model = PhoneChecksModel(
        text: PhoneChecks.text, build: ForgeBuildInfo.current.build)

    var body: some View {
        List {
            Section {
                Text(model.progress)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("checks.progress")
            }
            ForEach(model.groups) { group in
                Section(group.title) {
                    ForEach(group.items) { item in
                        CheckRow(model: model, item: item)
                    }
                }
            }
        }
        .groupedList()
        .navigationTitle(model.title)
        .inlineNavigationTitle()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                ShareLink(item: model.shareText) {
                    Label("Share Results", systemImage: "square.and.arrow.up")
                }
                .accessibilityIdentifier("checks.share")
            }
            ToolbarItem(placement: .secondaryAction) {
                Button(role: .destructive) {
                    model.clear()
                } label: {
                    Label("Clear Answers", systemImage: "arrow.counterclockwise")
                }
                .accessibilityIdentifier("checks.clear")
            }
        }
    }
}

private struct CheckRow: View {
    let model: PhoneChecksModel
    let item: PhoneChecksModel.Item

    var body: some View {
        let answer = model.answer(item)
        VStack(alignment: .leading, spacing: 8) {
            Text(item.text)
            HStack(spacing: 8) {
                choice(.pass, "Pass", "checkmark.circle", .success, answer)
                choice(.fail, "Fail", "xmark.circle", .danger, answer)
                choice(.skipped, "Skip", "forward.circle", .neutral, answer)
            }
            TextField("Note", text: noteBinding, axis: .vertical)
                .font(.footnote)
                .accessibilityIdentifier("checks.note")
        }
        .padding(.vertical, 4)
    }

    private var noteBinding: Binding<String> {
        Binding { model.answer(item).note } set: { model.setNote($0, for: item) }
    }

    private func choice(
        _ result: PhoneChecksModel.Result, _ title: String, _ symbol: String,
        _ tone: ForgeTone, _ answer: PhoneChecksModel.Answer
    ) -> some View {
        let chosen = answer.result == result
        return Button {
            model.set(result, for: item)
        } label: {
            Label(title, systemImage: chosen ? symbol + ".fill" : symbol)
                .font(.footnote)
        }
        .buttonStyle(.forgeChoice(isSelected: chosen, shape: .tile, tone: tone))
        .accessibilityIdentifier("checks.\(result.rawValue)")
    }
}
