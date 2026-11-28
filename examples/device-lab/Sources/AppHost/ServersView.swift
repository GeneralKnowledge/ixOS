import ForgeUI
import SwiftUI

/// Whether the hosted apps' servers answer from this phone, with the
/// address each app uses.
struct ServersView: View {
    @State private var model = ServersModel(servers: EmbeddedApps.servers())

    var body: some View {
        List {
            ForEach(model.servers) { server in
                Section {
                    ServerRow(server: server, check: model.check(server))
                } header: {
                    Text(server.app)
                }
            }
            Section {
            } footer: {
                Text("Each address is the one its app uses. Change it in the app. Addresses starting 100. are on Tailscale, so Tailscale must be on for this phone too.")
            }
        }
        .groupedList()
        .navigationTitle("Servers")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                ForgeButton("Check Again", systemImage: "arrow.clockwise") {
                    await model.checkAll()
                }
                .accessibilityIdentifier("servers.check")
            }
        }
        .task { await model.checkAll() }
        .refreshable { await model.checkAll() }
    }
}

private struct ServerRow: View {
    let server: ServersModel.Server
    let check: ServersModel.Check

    var body: some View {
        LabeledContent {
            status
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                Text(server.role)
                if let address = server.address {
                    Text(address + (ServersModel.isTailscale(address) ? " (Tailscale)" : ""))
                        .font(.footnote.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                } else {
                    Text(server.setUp)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("servers.\(server.id)")
    }

    @ViewBuilder private var status: some View {
        switch (server.address, check) {
        case (nil, _):
            Text("No address")
                .foregroundStyle(.secondary)
        case (_, .notChecked), (_, .checking):
            ProgressView()
        case let (_, .answered(ms)):
            Label("Up · \(ms) ms", systemImage: "checkmark.circle.fill")
                .forgeForeground(.success)
        case let (_, .unreachable(reason)):
            VStack(alignment: .trailing, spacing: 2) {
                Label("Down", systemImage: "xmark.circle.fill")
                    .forgeForeground(.danger)
                Text(reason)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.trailing)
            }
        }
    }
}
