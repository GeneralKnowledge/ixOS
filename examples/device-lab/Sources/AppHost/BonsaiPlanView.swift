@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

/// A plan (H4): the tasks Bonsai split the request into, each with the
/// tasks it waits for, its checks and its status, as in "2 / 3 tasks
/// passed". Shown before anything runs when approval is on; a plan that
/// failed or stopped resumes from its last verified task.
struct BonsaiPlanSection: View {
    let plan: HarnessPlan
    /// The plan as Bonsai writes it, before it parses.
    let draft: String
    let running: Bool
    let approve: @MainActor () -> Void
    let discard: @MainActor () -> Void

    var body: some View {
        Section {
            LabeledContent("Status", value: plan.status.title)
                .accessibilityIdentifier("harness.plan.status")
            if !plan.tasks.isEmpty {
                LabeledContent("Passed", value: "\(plan.passed) / \(plan.tasks.count) tasks")
                    .accessibilityIdentifier("harness.plan.passed")
            }
            if plan.status == .planning, !draft.isEmpty {
                Text(draft)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .lineLimit(12)
                    .accessibilityIdentifier("harness.plan.draft")
            }
            if !plan.thought.isEmpty {
                Text(plan.thought).font(.footnote)
            }
            ForEach(Array(plan.tasks.enumerated()), id: \.element.id) { index, task in
                PlanTaskRow(number: index + 1, task: task)
            }
            if let reason = plan.reason, plan.status != .awaitingApproval {
                Text(reason)
                    .font(.footnote)
                    .forgeForeground(plan.status == .completed ? .success : .danger)
                    .accessibilityIdentifier("harness.plan.result")
            }
            if !running {
                if plan.status == .awaitingApproval {
                    Button("Approve and Run", systemImage: "play.fill") { approve() }
                        .accessibilityIdentifier("harness.plan.approve")
                    Button("Discard Plan", systemImage: "trash", role: .destructive) { discard() }
                        .accessibilityIdentifier("harness.plan.discard")
                } else if plan.canContinue {
                    Button("Resume from Last Verified Task", systemImage: "arrow.clockwise") { approve() }
                        .accessibilityIdentifier("harness.plan.resume")
                }
            }
        } header: {
            Text("Plan: \(plan.request)")
        } footer: {
            if !plan.acceptance.isEmpty {
                Text("Once every task passes: \(plan.acceptance.joined(separator: "; ")).")
            } else if plan.canContinue, plan.status != .awaitingApproval {
                Text("Resuming puts the project's files back as the last verified task left them.")
            }
        }
    }
}

private struct PlanTaskRow: View {
    let number: Int
    let task: HarnessPlanTask

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Image(systemName: icon)
                    .forgeForeground(tone)
                    .accessibilityHidden(true)
                Text("\(number). \(task.id)").font(.subheadline.weight(.semibold))
                Spacer()
                Text(task.status.title).font(.caption).foregroundStyle(.secondary)
            }
            Text(task.description).font(.footnote)
            if !task.after.isEmpty {
                Text("After \(task.after.joined(separator: ", "))")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Text("Done when \(task.acceptance.joined(separator: "; ")).")
                .font(.caption).foregroundStyle(.secondary)
            if let result = task.result {
                Text(result)
                    .font(.caption)
                    .forgeForeground(task.status == .completed ? .success : .danger)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("harness.plan.task.\(task.id)")
    }

    private var icon: String {
        switch task.status {
        case .completed: "checkmark.circle.fill"
        case .failed: "xmark.circle.fill"
        case .stopped: "stop.circle"
        case .running, .verifying: "circle.dotted"
        case .ready: "circle"
        }
    }

    private var tone: ForgeTone {
        switch task.status {
        case .completed: .success
        case .failed: .danger
        default: .neutral
        }
    }
}
