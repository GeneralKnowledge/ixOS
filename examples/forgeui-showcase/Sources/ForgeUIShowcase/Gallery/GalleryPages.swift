#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

struct GalleryPageView: View {
    let page: GalleryPage

    var body: some View {
        switch page {
        case .colors: ColorsPage()
        case .layout: LayoutPage()
        case .typography: TypographyPage()
        case .buttons: ButtonsPage()
        case .iconButtons: IconButtonsPage()
        case .textFields: TextFieldsPage()
        case .formFields: FormFieldsPage()
        case .cards: CardsPage()
        case .rows: RowsPage()
        case .badges: BadgesPage()
        case .headers: HeadersPage()
        case .states: StatesPage()
        case .dialogs: DialogsPage()
        }
    }
}

/// A labelled picker over a CaseIterable enum, for page options.
private struct OptionPicker<Value: Hashable & Identifiable>: View {
    let title: String
    let id: String
    let values: [Value]
    let label: (Value) -> String
    @Binding var selection: Value

    var body: some View {
        Picker(title, selection: $selection) {
            ForEach(values) { Text(label($0)).tag($0) }
        }
        .accessibilityIdentifier(id)
    }
}

extension ForgeButtonStyle.Kind {
    var name: String {
        switch self {
        case .primary: "primary"
        case .secondary: "secondary"
        case .tertiary: "tertiary"
        case .destructive: "destructive"
        }
    }
}

// MARK: - Foundations

private struct ColorsPage: View {
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        let issues = theme.contrastIssues()
        ComponentPage(title: "Colour", code: """
        @Environment(\\.forgeTheme) var theme
        Text("Due today").foregroundStyle(theme.colors.tone(.warning).text)

        // An app's own accent, checked for contrast:
        let brand = ForgeTheme(palette: .standard.withAccent(light: ..., dark: ...))
        assert(brand.contrastIssues().isEmpty)
        """) {
            VStack(alignment: .leading, spacing: theme.spacing.m) {
                if issues.isEmpty {
                    ForgeStatus("This theme passes WCAG AA contrast in light and dark", tone: .success)
                } else {
                    ForgeStatus("\(issues.count) pairings fall below WCAG AA", tone: .danger)
                }
                swatchRow("Background", theme.colors.background)
                swatchRow("Grouped background", theme.colors.groupedBackground)
                swatchRow("Surface", theme.colors.surface)
                swatchRow("Text, primary", theme.colors.textPrimary)
                swatchRow("Text, secondary", theme.colors.textSecondary)
                swatchRow("Text, tertiary", theme.colors.textTertiary)
                swatchRow("Separator", theme.colors.separator)
                Divider()
                ForEach(ForgeTone.allCases) { tone in
                    HStack(spacing: theme.spacing.s) {
                        swatch(theme.colors.tone(tone).solid)
                        swatch(theme.colors.tone(tone).soft)
                        Text(tone.rawValue.capitalized)
                            .foregroundStyle(theme.colors.tone(tone).text)
                            .font(.body.weight(.semibold))
                        Spacer()
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        }
    }

    private func swatch(_ color: Color) -> some View {
        RoundedRectangle(cornerRadius: theme.radius.s, style: .continuous)
            .fill(color)
            .overlay(RoundedRectangle(cornerRadius: theme.radius.s, style: .continuous).strokeBorder(theme.colors.separator))
            .frame(width: 32, height: 32)
            .accessibilityHidden(true)
    }

    private func swatchRow(_ name: String, _ color: Color) -> some View {
        HStack(spacing: theme.spacing.m) {
            swatch(color)
            Text(name)
            Spacer()
        }
    }
}

private struct LayoutPage: View {
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        let steps: [(String, Double)] = [
            ("xxs", theme.spacing.xxs), ("xs", theme.spacing.xs), ("s", theme.spacing.s), ("m", theme.spacing.m),
            ("l", theme.spacing.l), ("xl", theme.spacing.xl), ("xxl", theme.spacing.xxl),
        ]
        ComponentPage(title: "Spacing and Corners", code: """
        .padding(theme.spacing.l)
        RoundedRectangle(cornerRadius: theme.radius.m, style: .continuous)
        """) {
            VStack(alignment: .leading, spacing: theme.spacing.s) {
                ForEach(steps, id: \.0) { step in
                    HStack {
                        Text(step.0).font(.footnote.monospaced()).frame(width: 44, alignment: .leading)
                        Rectangle()
                            .fill(theme.colors.accent.solid)
                            .frame(width: step.1 * 4, height: 12)
                        Text("\(Int(step.1)) pt").font(.footnote).foregroundStyle(theme.colors.textSecondary)
                    }
                }
                Divider()
                HStack(spacing: theme.spacing.l) {
                    ForEach([("s", theme.radius.s), ("m", theme.radius.m), ("l", theme.radius.l)], id: \.0) { corner in
                        VStack {
                            RoundedRectangle(cornerRadius: corner.1, style: .continuous)
                                .fill(theme.colors.accent.soft)
                                .frame(width: 64, height: 64)
                            Text("radius.\(corner.0)").font(.footnote.monospaced())
                        }
                    }
                }
            }
        }
    }
}

private struct TypographyPage: View {
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        let styles: [(String, Font)] = [
            ("largeTitle", .largeTitle), ("title", .title), ("title2", .title2), ("title3", .title3),
            ("headline", .headline), ("body", .body), ("callout", .callout), ("subheadline", .subheadline),
            ("footnote", .footnote), ("caption", .caption),
        ]
        ComponentPage(title: "Type", code: """
        // ForgeUI has no type ramp of its own: components use the
        // system text styles, so Dynamic Type and Bold Text just work.
        Text("Title").font(.headline)
        """) {
            VStack(alignment: .leading, spacing: theme.spacing.s) {
                ForEach(styles, id: \.0) { style in
                    Text(style.0).font(style.1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }
}

// MARK: - Controls

private struct ButtonsPage: View {
    @State private var kind = ForgeButtonStyle.Kind.primary
    @State private var compact = false
    @State private var expands = false
    @State private var disabled = false
    @State private var working = false
    @State private var taps = 0

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Buttons", code: """
        Button("Save", action: save)
            .buttonStyle(.forge(.\(kind.name)\(expands ? ", expands: true" : "")))\(compact ? "\n    .controlSize(.small)" : "")\(disabled ? "\n    .disabled(true)" : "")

        // A spinner in place of the title while saving:
        ForgeButton("Save", isWorking: model.isSaving, action: save)
            .buttonStyle(.forgePrimary)
        """) {
            VStack(spacing: theme.spacing.l) {
                ForgeButton("Save Changes", systemImage: "checkmark", role: kind == .destructive ? .destructive : nil, isWorking: working) {
                    taps += 1
                }
                .buttonStyle(.forge(kind, expands: expands))
                .controlSize(compact ? .small : .regular)
                .disabled(disabled)
                .accessibilityIdentifier("gallery.buttons.demo")
                Text("Tapped \(taps) times")
                    .font(.footnote)
                    .foregroundStyle(theme.colors.textSecondary)
                Divider()
                ForEach(ForgeButtonStyle.Kind.allCases) { each in
                    Button(each.name.capitalized) { taps += 1 }
                        .buttonStyle(.forge(each, expands: true))
                        .accessibilityIdentifier("gallery.buttons.\(each.name)")
                }
            }
        } options: {
            OptionPicker(title: "Style", id: "gallery.buttons.style", values: ForgeButtonStyle.Kind.allCases, label: { $0.name.capitalized }, selection: $kind)
            Toggle("Compact", isOn: $compact).accessibilityIdentifier("gallery.buttons.compact")
            Toggle("Fill width", isOn: $expands).accessibilityIdentifier("gallery.buttons.expands")
            Toggle("Disabled", isOn: $disabled).accessibilityIdentifier("gallery.buttons.disabled")
            Toggle("Working", isOn: $working).accessibilityIdentifier("gallery.buttons.working")
        }
    }
}

private struct IconButtonsPage: View {
    @State private var tone = ForgeTone.accent

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Icon Buttons", code: """
        Button { add() } label: { Label("Add", systemImage: "plus") }
            .buttonStyle(.forgeIcon(tone: .\(tone.rawValue)))
        // VoiceOver reads the label's title: "Add, button".
        """) {
            HStack(spacing: theme.spacing.l) {
                Button {} label: { Label("Add", systemImage: "plus") }
                    .accessibilityIdentifier("gallery.icon.add")
                Button {} label: { Label("Share", systemImage: "square.and.arrow.up") }
                    .accessibilityIdentifier("gallery.icon.share")
                Button {} label: { Label("Favourite", systemImage: "star.fill") }
                    .accessibilityIdentifier("gallery.icon.favourite")
                Button {} label: { Label("More", systemImage: "ellipsis") }
                    .accessibilityIdentifier("gallery.icon.more")
            }
            .buttonStyle(.forgeIcon(tone: tone))
        } options: {
            OptionPicker(title: "Tone", id: "gallery.icon.tone", values: ForgeTone.allCases, label: { $0.rawValue.capitalized }, selection: $tone)
        }
    }
}

private struct TextFieldsPage: View {
    @State private var email = ""
    @State private var password = ""
    @State private var disabled = false

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Text Fields", code: """
        TextField("Email", text: $email)
            .textFieldStyle(.forge)
        SecureField("Password", text: $password)
            .textFieldStyle(.forge)
        // In a Form, keep the default style: the row is the frame.
        // Search: SwiftUI's .searchable (see Screens > Tasks).
        """) {
            VStack(spacing: theme.spacing.m) {
                TextField("Email", text: $email)
                    #if os(iOS)
                    .textContentType(.emailAddress)
                    #endif
                    .autocorrectionDisabled()
                    .accessibilityIdentifier("gallery.fields.email")
                SecureField("Password", text: $password)
                    .accessibilityIdentifier("gallery.fields.password")
            }
            .textFieldStyle(.forge)
            .disabled(disabled)
        } options: {
            Toggle("Disabled", isOn: $disabled).accessibilityIdentifier("gallery.fields.disabled")
        }
    }
}

private struct FormFieldsPage: View {
    enum Message: String, CaseIterable, Identifiable {
        case none, help, success, warning, error
        var id: String { rawValue }

        var value: ForgeFieldMessage? {
            switch self {
            case .none: nil
            case .help: .help("Shown at the top of the list.")
            case .success: .success("That name is free.")
            case .warning: .warning("Long names are cut short in the list.")
            case .error: .error("Give the list a name.")
            }
        }
    }

    @State private var name = ""
    @State private var message = Message.error

    var body: some View {
        ComponentPage(title: "Form Fields", code: """
        ForgeField("Name", message: model.nameMessage) {
            TextField("Groceries", text: $model.name)
        }
        // model.nameMessage is a ForgeFieldMessage: Foundation only, so the
        // view model makes it (from a Rust validation result, say) and a
        // Linux test checks it.
        """) {
            ForgeField("Name", message: message.value) {
                TextField("Groceries", text: $name)
                    .textFieldStyle(.forge)
                    .accessibilityIdentifier("gallery.formField.name")
            }
        } options: {
            OptionPicker(title: "Message", id: "gallery.formField.message", values: Message.allCases, label: { $0.rawValue.capitalized }, selection: $message)
        }
    }
}

// MARK: - Content

private struct CardsPage: View {
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Cards", code: """
        ForgeCard {
            Text("This week").font(.headline)
            Text("4 of 6 tasks done")
        }
        GroupBox("Reading") { ... }
            .groupBoxStyle(.forgeCard)
        """) {
            VStack(spacing: theme.spacing.l) {
                ForgeCard {
                    Text("This week").font(.headline)
                    Text("4 of 6 tasks done").foregroundStyle(theme.colors.textSecondary)
                    ProgressView(value: 4, total: 6)
                }
                GroupBox("Reading") {
                    HStack {
                        ForgeStatus("On track", tone: .success)
                        Spacer()
                        ForgeBadge("12 days", tone: .accent)
                    }
                }
                .groupBoxStyle(.forgeCard)
            }
            .background(theme.colors.groupedBackground)
        }
    }
}

private struct RowsPage: View {
    enum Trailing: String, CaseIterable, Identifiable {
        case none, value, badge, status
        var id: String { rawValue }
    }

    @State private var icon = true
    @State private var subtitle = true
    @State private var tone = ForgeTone.accent
    @State private var trailing = Trailing.value

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Rows", code: """
        NavigationLink { ProfileView() } label: {
            ForgeRow("Your profile", subtitle: "Personal information",
                     systemImage: "person.fill")
        }
        Toggle(isOn: $notifications) {
            ForgeRow("Notifications", systemImage: "bell.badge.fill", tone: .danger)
        }
        ForgeRow("Version", value: "1.0.0")
        ForgeRow(task.title) { ForgeBadge("High", tone: .danger) }
        """) {
            ForgeRow(
                "Notifications",
                subtitle: subtitle ? "Reminders and updates" : nil,
                systemImage: icon ? "bell.badge.fill" : nil,
                tone: tone
            ) {
                switch trailing {
                case .none: EmptyView()
                case .value: Text("Enabled")
                case .badge: ForgeBadge("New", tone: .info)
                case .status: ForgeStatus("On", tone: .success)
                }
            }
        } options: {
            Toggle("Icon", isOn: $icon).accessibilityIdentifier("gallery.rows.icon")
            Toggle("Subtitle", isOn: $subtitle).accessibilityIdentifier("gallery.rows.subtitle")
            OptionPicker(title: "Icon tone", id: "gallery.rows.tone", values: ForgeTone.allCases, label: { $0.rawValue.capitalized }, selection: $tone)
            OptionPicker(title: "Trailing", id: "gallery.rows.trailing", values: Trailing.allCases, label: { $0.rawValue.capitalized }, selection: $trailing)
        }
    }
}

private struct BadgesPage: View {
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Badges and Status", code: """
        ForgeBadge("Overdue", tone: .warning)
        ForgeBadge("3 new", systemImage: "sparkles", tone: .info)
        ForgeStatus("Synced", tone: .success)
        """) {
            VStack(alignment: .leading, spacing: theme.spacing.m) {
                ForEach(ForgeTone.allCases) { tone in
                    HStack {
                        ForgeBadge(tone.rawValue.capitalized, tone: tone)
                        Spacer()
                        ForgeStatus(statusText(tone), tone: tone)
                    }
                }
                Divider()
                ForgeBadge("3 new", systemImage: "sparkles", tone: .info)
            }
        }
    }

    private func statusText(_ tone: ForgeTone) -> String {
        switch tone {
        case .accent: "Selected"
        case .neutral: "Draft"
        case .success: "Synced"
        case .warning: "Due soon"
        case .danger: "Failed"
        case .info: "Updating"
        }
    }
}

private struct HeadersPage: View {
    @State private var tone = ForgeTone.accent
    @State private var seeAllTaps = 0

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Headers", code: """
        ForgeSectionHeader("Recent", actionTitle: "See All") { showAll() }
        ForgeDetailHeader(book.title, subtitle: book.author,
                          systemImage: "book.closed.fill") {
            ForgeBadge("Reading", tone: .accent)
        }
        """) {
            VStack(spacing: theme.spacing.l) {
                ForgeSectionHeader("Recent", actionTitle: "See All") { seeAllTaps += 1 }
                    .accessibilityIdentifier("gallery.headers.seeAll")
                ForgeDetailHeader("The Left Hand of Darkness", subtitle: "Ursula K. Le Guin", systemImage: "book.closed.fill", tone: tone) {
                    ForgeBadge("Reading", tone: .accent)
                    ForgeBadge("Library copy", tone: .neutral)
                }
            }
        } options: {
            OptionPicker(title: "Tone", id: "gallery.headers.tone", values: ForgeTone.allCases, label: { $0.rawValue.capitalized }, selection: $tone)
        }
    }
}

// MARK: - Feedback

private struct StatesPage: View {
    enum Kind: String, CaseIterable, Identifiable {
        case empty, loading, failed, noResults
        var id: String { rawValue }
        var title: String {
            switch self {
            case .empty: "Empty"
            case .loading: "Loading"
            case .failed: "Failed"
            case .noResults: "No results"
            }
        }
    }

    @State private var kind = Kind.empty

    var body: some View {
        ComponentPage(title: "Empty, Loading, Error", code: """
        ForgeStateView.empty("No Tasks", systemImage: "checklist",
            description: "Tasks you add appear here.",
            action: ForgeStateAction("Add Task", identifier: "tasks.add") { ... })
        ForgeStateView.loading("Loading tasks")
        ForgeStateView.failed("Couldn't Load Tasks", message: error,
            retry: ForgeStateAction("Try Again", identifier: "tasks.retry") { ... })
        ForgeStateView.noResults(for: query)
        """) {
            Group {
                switch kind {
                case .empty:
                    ForgeStateView.empty("No Tasks", systemImage: "checklist", description: "Tasks you add appear here.",
                                         action: ForgeStateAction("Add Task", identifier: "gallery.states.add") {})
                case .loading:
                    ForgeStateView.loading("Loading tasks")
                case .failed:
                    ForgeStateView.failed("Couldn't Load Tasks", message: "The server didn't answer. Check your connection and try again.",
                                          retry: ForgeStateAction("Try Again", identifier: "gallery.states.retry") {})
                case .noResults:
                    ForgeStateView.noResults(for: "passport")
                }
            }
            .frame(height: 320)
        } options: {
            OptionPicker(title: "State", id: "gallery.states.kind", values: Kind.allCases, label: { $0.title }, selection: $kind)
        }
    }
}

private struct DialogsPage: View {
    @State private var showingAlert = false
    @State private var confirmingDelete = false
    @State private var showingSheet = false
    @State private var outcome = "Nothing yet"

    @Environment(\.forgeTheme) private var theme

    var body: some View {
        ComponentPage(title: "Alerts, Dialogs, Sheets", code: """
        // ForgeUI doesn't wrap these: SwiftUI's own, tinted by the theme.
        .alert("Couldn't Save", isPresented: $failed) { ... }
        .confirmationDialog("Delete this task?", isPresented: $confirming) {
            Button("Delete Task", role: .destructive, action: delete)
        }
        .sheet(isPresented: $editing) {
            NavigationStack {
                Form { ... }
                    .forgeFormToolbar(id: "taskForm", canConfirm: model.canSave,
                                      isWorking: model.isSaving,
                                      cancel: { dismiss() }, confirm: save)
            }
        }
        """) {
            VStack(spacing: theme.spacing.m) {
                Button("Show Alert") { showingAlert = true }
                    .buttonStyle(.forge(.secondary, expands: true))
                    .accessibilityIdentifier("gallery.dialogs.alert")
                Button("Show Confirmation") { confirmingDelete = true }
                    .buttonStyle(.forge(.destructive, expands: true))
                    .accessibilityIdentifier("gallery.dialogs.confirm")
                Button("Show Sheet") { showingSheet = true }
                    .buttonStyle(.forge(.primary, expands: true))
                    .accessibilityIdentifier("gallery.dialogs.sheet")
                ForgeStatus(outcome, tone: .neutral)
            }
            .alert("Couldn't Save", isPresented: $showingAlert) {
                Button("OK") { outcome = "Alert dismissed" }
                    .accessibilityIdentifier("alert.ok")
            } message: {
                Text("The task list is read-only in the showcase.")
            }
            .confirmationDialog("Delete this task?", isPresented: $confirmingDelete, titleVisibility: .visible) {
                Button("Delete Task", role: .destructive) { outcome = "Deleted" }
                    .accessibilityIdentifier("gallery.dialogs.delete")
                Button("Cancel", role: .cancel) { outcome = "Kept" }
            } message: {
                Text("This can't be undone.")
            }
            .sheet(isPresented: $showingSheet) {
                SampleSheet { outcome = $0 }
            }
        }
    }
}

private struct SampleSheet: View {
    let finished: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var saving = false

    var body: some View {
        NavigationStack {
            Form {
                ForgeField("List name", message: name.isEmpty ? .help("Required") : nil) {
                    TextField("Groceries", text: $name)
                        .accessibilityIdentifier("sheet.name")
                }
            }
            .forgeGroupedBackground()
            .navigationTitle("New List")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .forgeFormToolbar(id: "sheet", confirmTitle: "Add", canConfirm: !name.isEmpty, isWorking: saving, cancel: {
                finished("Sheet cancelled")
                dismiss()
            }, confirm: {
                saving = true
                Task {
                    try? await Task.sleep(for: .milliseconds(700))
                    finished("Added “\(name)”")
                    dismiss()
                }
            })
        }
    }
}
#endif
