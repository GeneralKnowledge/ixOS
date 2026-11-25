@preconcurrency import RelayFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// New Group: a name and members picked from the trusted devices. Each
/// message then goes once to each member, over Relay's encrypted device
/// connections, with no server in between (docs/relay-groups.md).
struct NewGroupView: View {
    let model: ChatModel
    let created: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var picked: Set<String> = []
    @State private var working = false
    @State private var error: ForgeFailure?
    @FocusState private var focused: Bool

    private var trimmed: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var canCreate: Bool { !trimmed.isEmpty && !picked.isEmpty }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Group Name", text: $name)
                        .focused($focused)
                        .accessibilityIdentifier("newGroup.name")
                        .submitLabel(.done)
                }
                Section {
                    let candidates = model.groupCandidates()
                    if candidates.isEmpty {
                        Text("Trust a device or a person's device first, from Devices.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(candidates, id: \.endpointId) { device in
                        MemberPickRow(
                            name: device.name, person: device.person,
                            picked: picked.contains(device.endpointId)
                        ) {
                            toggle(device.endpointId)
                        }
                        .accessibilityIdentifier("newGroup.member.\(device.endpointId)")
                    }
                } header: {
                    Text("Members")
                } footer: {
                    Text("Only the members' devices can read the group. You can add and remove members later.")
                }
            }
            .navigationTitle("New Group")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(
                id: "newGroup", confirmTitle: "Create", canConfirm: canCreate,
                isWorking: working, hasChanges: !trimmed.isEmpty || !picked.isEmpty,
                cancel: { dismiss() }, confirm: create
            )
            .forgeAlert(failure: $error)
            .onAppear { focused = true }
        }
    }

    private func toggle(_ id: String) {
        if picked.contains(id) { picked.remove(id) } else { picked.insert(id) }
    }

    private func create() {
        guard canCreate, !working else { return }
        working = true
        Task {
            defer { working = false }
            do {
                created(try await model.createGroup(name: trimmed, members: Array(picked)))
            } catch {
                self.error = ChatModel.failure(error, title: "Can't Create the Group")
            }
        }
    }
}

/// A device to pick, with a checkmark when picked.
private struct MemberPickRow: View {
    let name: String
    let person: String?
    let picked: Bool
    let toggle: () -> Void

    var body: some View {
        Button(action: toggle) {
            HStack {
                VStack(alignment: .leading) {
                    Text(name).foregroundStyle(.primary)
                    if let person {
                        Text("\(person)'s").font(.caption).foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if picked {
                    Image(systemName: "checkmark").foregroundStyle(.tint)
                }
            }
            .contentShape(Rectangle())
        }
        .accessibilityAddTraits(picked ? .isSelected : [])
    }
}

/// A group's members, and for its admin Add Members, Remove and Rename;
/// Leave Group for everyone.
struct GroupInfoView: View {
    let model: ChatModel
    let conversationId: String
    @Environment(\.dismiss) private var dismiss
    @State private var group: RelayGroup?
    @State private var loaded = false
    @State private var adding = false
    @State private var renaming = false
    @State private var newName = ""
    @State private var confirmingLeave = false

    var body: some View {
        NavigationStack {
            content
                .navigationTitle(group?.name ?? String(localized: "Group"))
                #if os(iOS)
                    .navigationBarTitleDisplayMode(.inline)
                #endif
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { dismiss() }
                            .accessibilityIdentifier("groupInfo.done")
                    }
                }
                .task { await reload() }
                .sheet(isPresented: $adding) {
                    if let group {
                        AddMembersView(model: model, group: group) { updated in
                            self.group = updated
                            adding = false
                        }
                    }
                }
                .alert("Rename Group", isPresented: $renaming) {
                    TextField("Group Name", text: $newName)
                        .accessibilityIdentifier("groupInfo.renameField")
                    Button("Cancel", role: .cancel) {}
                    Button("Rename") { rename() }
                        .accessibilityIdentifier("groupInfo.renameConfirm")
                }
                .confirmationDialog(
                    "Leave this group?", isPresented: $confirmingLeave, titleVisibility: .visible
                ) {
                    Button("Leave Group", role: .destructive) { leave() }
                        .accessibilityIdentifier("groupInfo.leaveConfirm")
                } message: {
                    Text("You won't get its messages any more. The conversation stays on this device.")
                }
        }
    }

    @ViewBuilder private var content: some View {
        if let group {
            List {
                if group.left {
                    Section {
                        Text("You're no longer in this group. Its messages stay here, read-only.")
                            .foregroundStyle(.secondary)
                    }
                }
                Section {
                    ForEach(group.members, id: \.endpointId) { member in
                        MemberRow(member: member, isAdmin: member.endpointId == group.admin)
                            .accessibilityIdentifier("groupInfo.member.\(member.endpointId)")
                            .swipeActions {
                                if group.isAdmin && !group.left && !member.isMe {
                                    Button("Remove", role: .destructive) { remove(member) }
                                        .accessibilityIdentifier("groupInfo.remove.\(member.endpointId)")
                                }
                            }
                    }
                } header: {
                    Text("\(group.members.count) Members")
                } footer: {
                    Text("Each message goes straight to each member's device, encrypted on the way. No server keeps it.")
                }
                if group.isAdmin && !group.left {
                    Section {
                        Button("Add Members", systemImage: "person.badge.plus") { adding = true }
                            .accessibilityIdentifier("groupInfo.add")
                        Button("Rename Group", systemImage: "pencil") {
                            newName = group.name
                            renaming = true
                        }
                        .accessibilityIdentifier("groupInfo.rename")
                    }
                }
                if !group.left {
                    Section {
                        Button("Leave Group", systemImage: "rectangle.portrait.and.arrow.right", role: .destructive) {
                            confirmingLeave = true
                        }
                        .accessibilityIdentifier("groupInfo.leave")
                    }
                }
            }
        } else if loaded {
            ForgeStateView.failed("Can't Open the Group", message: "This group isn't on this device any more.")
        } else {
            ForgeStateView.loading()
        }
    }

    private func reload() async {
        group = await model.group(conversationId)
        loaded = true
    }

    private func remove(_ member: RelayGroupMember) {
        guard let group else { return }
        Task {
            if let updated = await model.removeGroupMember(group, member.endpointId) {
                self.group = updated
            }
        }
    }

    private func rename() {
        guard let group else { return }
        let name = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        Task {
            if let updated = await model.renameGroup(group, name) {
                self.group = updated
            }
        }
    }

    private func leave() {
        guard let group else { return }
        Task {
            await model.leaveGroup(group)
            await reload()
        }
    }
}

/// One member: its name, and whether it is you, the admin, or a device
/// this one hasn't paired with (skipped for now).
private struct MemberRow: View {
    let member: RelayGroupMember
    let isAdmin: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(member.isMe ? String(localized: "\(member.name) (You)") : member.name)
            if let detail {
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var detail: String? {
        var parts: [String] = []
        if isAdmin { parts.append(String(localized: "Admin")) }
        if !member.isMe && !member.trusted {
            parts.append(String(localized: "Not paired with this iPhone: messages don't go to it yet"))
        }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
}

/// Add Members: trusted devices not in the group yet.
private struct AddMembersView: View {
    let model: ChatModel
    let group: RelayGroup
    let added: (RelayGroup) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var picked: Set<String> = []
    @State private var working = false

    var body: some View {
        NavigationStack {
            Form {
                let inGroup = Set(group.members.map(\.endpointId))
                let candidates = model.groupCandidates().filter { !inGroup.contains($0.endpointId) }
                if candidates.isEmpty {
                    Text("Every trusted device is in this group already.")
                        .foregroundStyle(.secondary)
                }
                ForEach(candidates, id: \.endpointId) { device in
                    MemberPickRow(
                        name: device.name, person: device.person,
                        picked: picked.contains(device.endpointId)
                    ) {
                        if picked.contains(device.endpointId) {
                            picked.remove(device.endpointId)
                        } else {
                            picked.insert(device.endpointId)
                        }
                    }
                    .accessibilityIdentifier("addMembers.member.\(device.endpointId)")
                }
            }
            .navigationTitle("Add Members")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(
                id: "addMembers", confirmTitle: "Add", canConfirm: !picked.isEmpty,
                isWorking: working, hasChanges: !picked.isEmpty, cancel: { dismiss() }, confirm: add
            )
        }
    }

    private func add() {
        guard !picked.isEmpty, !working else { return }
        working = true
        Task {
            defer { working = false }
            if let updated = await model.addGroupMembers(group, Array(picked)) {
                added(updated)
            }
        }
    }
}
