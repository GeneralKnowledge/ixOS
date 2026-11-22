@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

/// Bonsai's current step as it writes it: the task as the title, the
/// thought above, then the file it is writing, streaming in line by line.
/// Rust reads the half-written answer (`HarnessLive`); this only draws it.
struct BonsaiLiveSection: View {
    let task: String
    let step: Int
    let phase: HarnessPhase
    let live: HarnessLive?

    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 10) {
                Label(stepTitle, systemImage: icon)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .contentTransition(.opacity)
                    .accessibilityIdentifier("harness.live.step")
                thought
                if let live, !live.replacing.isEmpty {
                    LiveCode(text: live.replacing, writing: false, removed: true)
                        .accessibilityLabel("Replacing: \(live.replacing)")
                }
                if let live, !live.code.isEmpty {
                    LiveCode(text: live.code, writing: live.writing, removed: false)
                        .accessibilityIdentifier("harness.live.code")
                }
            }
            .padding(.vertical, 4)
            .animation(.default, value: live?.tool)
        } header: {
            Text(task)
                .font(.headline)
                .foregroundStyle(.primary)
                .textCase(nil)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("harness.live.task")
        }
    }

    @ViewBuilder
    private var thought: some View {
        if let live, !live.thought.isEmpty {
            (Text(live.thought) + Text(live.thoughtDone ? "" : " ▍").foregroundStyle(.tint))
                .font(.callout.italic())
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityLabel("Thinking: \(live.thought)")
                .accessibilityIdentifier("harness.live.thought")
        } else if phase == .thinking {
            HStack(spacing: 8) {
                ProgressView()
                Text("Bonsai is reading the task…").font(.callout).foregroundStyle(.secondary)
            }
            .accessibilityIdentifier("harness.live.waiting")
        }
    }

    private var stepTitle: String {
        let what: String = switch (live?.tool, live?.path) {
        case ("write_file"?, let path?): "Writing \(path)"
        case ("write_file"?, nil): "Writing a file"
        case ("edit_file"?, let path?): "Editing \(path)"
        case ("edit_file"?, nil): "Editing a file"
        case ("read_file"?, let path?): "Reading \(path)"
        case ("run_python"?, let path?): "Running \(path)"
        case ("run_tests"?, _): "Running the tests"
        case ("list_files"?, _): "Looking at the project"
        case ("finish"?, _): "Saying it's done"
        case (let tool?, _): tool
        case (nil, _): "Thinking"
        }
        let running = phase == .acting ? " (running)" : phase == .verifying ? " (checking the evidence)" : ""
        return "Step \(step): \(what)\(running)"
    }

    private var icon: String {
        switch live?.tool {
        case "write_file", "edit_file": "pencil.line"
        case "read_file", "list_files": "doc.text.magnifyingglass"
        case "run_python", "run_tests": "play.circle"
        case "finish": "checkmark.seal"
        default: "brain"
        }
    }
}

/// Code with line numbers that follows the end as it grows.
private struct LiveCode: View {
    let text: String
    let writing: Bool
    let removed: Bool

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView([.vertical, .horizontal]) {
                VStack(alignment: .leading, spacing: 0) {
                    numbered
                        .font(.system(.caption, design: .monospaced))
                        .fixedSize(horizontal: true, vertical: false)
                        .textSelection(.enabled)
                    Color.clear.frame(height: 1).id("end")
                }
                .padding(10)
            }
            .frame(maxHeight: removed ? 90 : 280)
            .defaultScrollAnchor(.bottom)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
            .onChange(of: text) {
                proxy.scrollTo("end", anchor: .bottom)
            }
        }
    }

    /// The lines with their numbers in a quieter colour, and a caret at the
    /// end while Bonsai is still writing.
    private var numbered: Text {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        let width = String(lines.count).count
        var out = Text("")
        for (index, line) in lines.enumerated() {
            let label = String(index + 1)
            let number = String(repeating: " ", count: width - label.count) + label + "  "
            var code = Text(String(line))
            if removed { code = code.strikethrough().foregroundStyle(.secondary) }
            out = out + Text(number).foregroundStyle(.tertiary) + code
            if index < lines.count - 1 { out = out + Text("\n") }
        }
        if writing { out = out + Text("▍").foregroundStyle(.tint) }
        return out
    }
}
