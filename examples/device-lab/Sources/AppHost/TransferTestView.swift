@preconcurrency import ForgeFFI
import SwiftUI

/// Background downloads through Forge's `transfers` service, for checking it
/// on a phone: start a download, leave the app, end it or swipe it away, and
/// come back to see what the service made of it.
struct TransferTestView: View {
    @Bindable var model: TransferTestModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        List {
            Section {
                Button("Download 20 MB") { model.start(megabytes: 20) }
                    .accessibilityIdentifier("transfers.start20")
                Button("Download 200 MB") { model.start(megabytes: 200) }
                    .accessibilityIdentifier("transfers.start200")
                Toggle("End the app when I leave", isOn: $model.endWhenBackgrounded)
                    .accessibilityIdentifier("transfers.endWhenBackgrounded")
            } footer: {
                Text(
                    "Downloads keep going after you leave the app. With the switch on, the app ends a second after you leave, the way iOS reclaims a suspended app; swiping it away in the app switcher is different, and stops its downloads."
                )
            }
            if let error = model.error {
                Section {
                    Text(error).foregroundStyle(.red)
                }
            }
            Section("Transfers") {
                if model.rows.isEmpty {
                    Text("None yet").foregroundStyle(.secondary)
                }
                ForEach(model.rows, id: \.id) { row in
                    TransferRowView(row: row, model: model)
                }
            }
            Section {
                Button("Clear Finished", role: .destructive) { model.clear() }
                    .accessibilityIdentifier("transfers.clear")
            }
        }
        .groupedList()
        .navigationTitle("Background Downloads")
        .inlineNavigationTitle()
        .task { await model.follow() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { model.didEnterBackground() }
        }
    }
}

private struct TransferRowView: View {
    let row: TransferRow
    let model: TransferTestModel

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(row.name).font(.subheadline.monospaced()).lineLimit(1)
                Spacer()
                Text(row.state).font(.subheadline.weight(.semibold))
            }
            if let fraction = row.fraction, row.state == "Running" {
                ProgressView(value: fraction)
            }
            if let detail = row.detail {
                Text(detail).font(.footnote).foregroundStyle(.secondary)
            }
            if row.attempts > 1 {
                Text("Attempt \(row.attempts)").font(.footnote).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("transfers.row")
        .swipeActions {
            if row.canCancel {
                Button("Cancel", role: .destructive) { model.cancel(row) }
                    .accessibilityIdentifier("transfers.cancel")
            }
            if row.canRetry {
                Button("Retry") { model.retry(row) }
                    .accessibilityIdentifier("transfers.retry")
            }
        }
    }
}
