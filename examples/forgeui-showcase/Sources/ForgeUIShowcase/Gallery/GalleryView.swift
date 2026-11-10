#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

/// Every page of the gallery. Adding a component means adding a case here
/// and its page in GalleryPages.swift.
enum GalleryPage: String, CaseIterable, Identifiable, Hashable {
    case colors, layout, typography
    case buttons, iconButtons, textFields, formFields
    case cards, rows, badges, headers
    case states, dialogs

    var id: String { rawValue }

    enum Group: String, CaseIterable {
        case foundations = "Foundations"
        case controls = "Controls"
        case content = "Content"
        case feedback = "Feedback"
    }

    var group: Group {
        switch self {
        case .colors, .layout, .typography: .foundations
        case .buttons, .iconButtons, .textFields, .formFields: .controls
        case .cards, .rows, .badges, .headers: .content
        case .states, .dialogs: .feedback
        }
    }

    var title: String {
        switch self {
        case .colors: "Colour"
        case .layout: "Spacing and Corners"
        case .typography: "Type"
        case .buttons: "Buttons"
        case .iconButtons: "Icon Buttons"
        case .textFields: "Text Fields"
        case .formFields: "Form Fields"
        case .cards: "Cards"
        case .rows: "Rows"
        case .badges: "Badges and Status"
        case .headers: "Headers"
        case .states: "Empty, Loading, Error"
        case .dialogs: "Alerts, Dialogs, Sheets"
        }
    }

    var summary: String {
        switch self {
        case .colors: "Roles, tones, contrast check"
        case .layout: "The spacing scale and corner radii"
        case .typography: "System text styles with Dynamic Type"
        case .buttons: "Primary, secondary, tertiary, destructive"
        case .iconButtons: "Round, icon-only actions"
        case .textFields: "Bordered fields outside a form"
        case .formFields: "Labels with help and validation"
        case .cards: "Raised, grouped surfaces"
        case .rows: "Settings and list rows"
        case .badges: "Tinted labels and status dots"
        case .headers: "Section and detail headers"
        case .states: "When there is nothing to show yet"
        case .dialogs: "Native presentation, themed"
        }
    }

    var systemImage: String {
        switch self {
        case .colors: "paintpalette.fill"
        case .layout: "ruler.fill"
        case .typography: "textformat"
        case .buttons: "button.horizontal.fill"
        case .iconButtons: "plus.circle.fill"
        case .textFields: "character.cursor.ibeam"
        case .formFields: "list.bullet.rectangle.fill"
        case .cards: "rectangle.stack.fill"
        case .rows: "list.bullet"
        case .badges: "tag.fill"
        case .headers: "textformat.size"
        case .states: "tray.fill"
        case .dialogs: "rectangle.portrait.on.rectangle.portrait.fill"
        }
    }

    var tone: ForgeTone {
        switch group {
        case .foundations: .accent
        case .controls: .info
        case .content: .success
        case .feedback: .warning
        }
    }
}

struct GalleryView: View {
    @Binding var theme: ShowcaseTheme
    @Binding var appearance: ShowcaseAppearance
    @Binding var textSize: ShowcaseTextSize

    var body: some View {
        NavigationStack {
            List {
                ForEach(GalleryPage.Group.allCases, id: \.self) { group in
                    Section(group.rawValue) {
                        ForEach(GalleryPage.allCases.filter { $0.group == group }) { page in
                            NavigationLink(value: page) {
                                ForgeRow(page.title, subtitle: page.summary, systemImage: page.systemImage, tone: page.tone)
                            }
                            .accessibilityIdentifier("gallery.\(page.rawValue)")
                        }
                    }
                }
            }
            .forgeGroupedBackground()
            .navigationTitle("ForgeUI")
            .navigationDestination(for: GalleryPage.self) { page in
                GalleryPageView(page: page)
                    .toolbar {
                        ToolbarItem(placement: .primaryAction) {
                            AppearanceMenu(theme: $theme, appearance: $appearance, textSize: $textSize)
                        }
                    }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    AppearanceMenu(theme: $theme, appearance: $appearance, textSize: $textSize)
                }
            }
        }
    }
}

/// The frame every gallery page shares: the component live on a stage, the
/// options that change it, and the code that makes it.
struct ComponentPage<Demo: View, Options: View>: View {
    let title: String
    let code: String
    let demo: () -> Demo
    let options: () -> Options

    @Environment(\.forgeTheme) private var theme

    init(title: String, code: String, @ViewBuilder demo: @escaping () -> Demo, @ViewBuilder options: @escaping () -> Options) {
        self.title = title
        self.code = code
        self.demo = demo
        self.options = options
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.l) {
                ForgeCard {
                    demo()
                        .frame(maxWidth: .infinity)
                }
                if Options.self != EmptyView.self {
                    ForgeSectionHeader("Options")
                    ForgeCard {
                        options()
                    }
                }
                ForgeSectionHeader("Code")
                ForgeCard {
                    Text(code)
                        .font(.footnote.monospaced())
                        .foregroundStyle(theme.colors.textSecondary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(theme.spacing.l)
        }
        .background(theme.colors.groupedBackground.ignoresSafeArea())
        .navigationTitle(title)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }
}

extension ComponentPage where Options == EmptyView {
    init(title: String, code: String, @ViewBuilder demo: @escaping () -> Demo) {
        self.init(title: title, code: code, demo: demo, options: { EmptyView() })
    }
}
#endif
