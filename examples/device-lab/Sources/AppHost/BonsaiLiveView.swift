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

/// Code with line numbers that follows the caret as it grows: down to the
/// newest line, right along a long line, and back to the left when Bonsai
/// starts a new one.
private struct LiveCode: View {
    let text: String
    let writing: Bool
    let removed: Bool

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView([.vertical, .horizontal]) {
                VStack(alignment: .leading, spacing: 0) {
                    if lines.count > 1 {
                        numbered(lines.dropLast(), from: 1)
                    }
                    // The last line carries the caret, so scrolling to the
                    // caret shows where Bonsai is writing now.
                    HStack(spacing: 0) {
                        numbered(lines.suffix(1), from: lines.count)
                        Text(writing ? "▍" : "")
                            .foregroundStyle(.tint)
                            .id("caret")
                    }
                }
                .font(.system(.caption, design: .monospaced))
                .fixedSize(horizontal: true, vertical: false)
                .textSelection(.enabled)
                .padding(10)
            }
            .frame(maxHeight: removed ? 90 : 280)
            .defaultScrollAnchor(.bottomLeading)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
            .onChange(of: text) {
                // Trailing edge to the caret: on a short new line the offset
                // can't go below zero, so the view slides back to the left.
                proxy.scrollTo("caret", anchor: .bottomTrailing)
            }
        }
    }

    private var lines: [String] {
        text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
    }

    /// Lines with their numbers in a quieter colour, padded to the width of
    /// the largest number.
    private func numbered(_ slice: some Collection<String>, from first: Int) -> Text {
        let width = String(lines.count).count
        var out = Text("")
        for (offset, line) in slice.enumerated() {
            let label = String(first + offset)
            let number = String(repeating: " ", count: width - label.count) + label + "  "
            var code = Text(line)
            if removed { code = code.strikethrough().foregroundStyle(.secondary) }
            if offset > 0 { out = out + Text("\n") }
            out = out + Text(number).foregroundStyle(.tertiary) + code
        }
        return out
    }
}
