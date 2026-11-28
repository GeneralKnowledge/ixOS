@preconcurrency import BusinessPreviewFFI
import ForgeUI
import SwiftUI
#if canImport(SafariServices) && canImport(UIKit)
    import SafariServices
#endif

/// One business's preview, full screen: the services-business template
/// (Home, Services, Visit) in the business's colours. Above Device Lab's own
/// controls, so the owner sees only their app.
struct PreviewRoot: View {
    @State private var model: PreviewModel
    @Environment(\.dismiss) private var dismiss

    init(core: BusinessPreviewCore, prospect: Prospect) {
        _model = State(initialValue: PreviewModel(core: core, id: prospect.id))
    }

    var body: some View {
        Group {
            switch model.state {
            case .loading:
                ForgeStateView.loading("Opening")
            case .failed(let failure):
                ForgeStateView.failed(
                    "Can't Show This Business",
                    message: failure.message,
                    retry: ForgeStateAction("Close", identifier: "preview.close") { dismiss() }
                )
            case .ready(let business):
                BusinessApp(business: business) { dismiss() }
            }
        }
        .task { await model.load() }
    }
}

/// A page opened in the in-app browser.
private struct WebPage: Identifiable {
    let url: URL
    var id: URL { url }
}

private enum PreviewTab: Hashable {
    case home, services, visit
}

/// The business's app. Outside presenter mode it has Done and Present
/// buttons; in presenter mode it has nothing of ours, and a long press
/// anywhere leaves it.
private struct BusinessApp: View {
    let business: BusinessView
    let close: () -> Void
    @State private var tab = PreviewTab.home
    @State private var presenting = false
    @State private var page: WebPage?
    @Environment(\.openURL) private var openURL

    var body: some View {
        TabView(selection: $tab) {
            screen { HomeScreen(business: business, act: act) }
                .tabItem { Label("Home", systemImage: "house") }
                .tag(PreviewTab.home)
            screen { ServicesScreen(business: business) }
                .tabItem { Label(business.words.servicesTitle, systemImage: "list.bullet.rectangle") }
                .tag(PreviewTab.services)
            screen { VisitScreen(business: business, act: act) }
                .tabItem { Label("Visit", systemImage: "mappin.and.ellipse") }
                .tag(PreviewTab.visit)
        }
        .forgeTheme(BrandTheme.theme(business.brand))
        .sheet(item: $page) { page in
            InAppBrowser(url: page.url)
                .ignoresSafeArea()
        }
        // Presenter mode: one long press anywhere leaves it.
        .simultaneousGesture(
            LongPressGesture(minimumDuration: 1.2).onEnded { _ in
                if presenting { presenting = false }
            },
            including: presenting ? .all : .subviews
        )
        .accessibilityAction(.escape) {
            if presenting { presenting = false } else { close() }
        }
    }

    /// One tab's stack, with the concept label and our controls.
    private func screen<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        NavigationStack {
            content()
                .safeAreaInset(edge: .top, spacing: 0) {
                    if let label = business.conceptLabel {
                        ConceptLabel(text: label)
                    }
                }
                .toolbar {
                    if !presenting {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done", action: close)
                                .accessibilityIdentifier("preview.done")
                        }
                        ToolbarItem(placement: .primaryAction) {
                            Button("Present", systemImage: "play.rectangle") { presenting = true }
                                .accessibilityIdentifier("preview.present")
                                .accessibilityHint("Hides these buttons. A long press anywhere brings them back.")
                        }
                    }
                }
        }
    }

    /// Opens a contact or the book button: their own page in the in-app
    /// browser, a call, an email or Maps through the system.
    private func act(_ url: String, inApp: Bool) {
        guard let url = URL(string: url) else { return }
        if inApp {
            page = WebPage(url: url)
        } else {
            openURL(url)
        }
    }
}

/// The label every screen of a concept preview carries (Rust decides when).
private struct ConceptLabel: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.footnote.weight(.semibold))
            .multilineTextAlignment(.center)
            .padding(.vertical, 6)
            .padding(.horizontal)
            .frame(maxWidth: .infinity)
            .background(.regularMaterial)
            .accessibilityIdentifier("preview.concept")
    }
}

/// The business's colours as a ForgeUI theme, so every control wears them.
enum BrandTheme {
    static func theme(_ brand: BrandColors?) -> ForgeTheme {
        guard let brand else { return ForgeTheme(accent: .blue) }
        let light = ForgeToneColors(
            solid: ForgeRGBA(hex: brand.lightSolid), onSolid: ForgeRGBA(hex: brand.lightOnSolid),
            text: ForgeRGBA(hex: brand.lightText), softOpacity: ForgePalette.lightSoft)
        let dark = ForgeToneColors(
            solid: ForgeRGBA(hex: brand.darkSolid), onSolid: ForgeRGBA(hex: brand.darkOnSolid),
            text: ForgeRGBA(hex: brand.darkText), softOpacity: ForgePalette.darkSoft)
        return ForgeTheme(palette: ForgePalette.standard.withAccent(light: light, dark: dark))
    }
}

/// A picture from the pack, decoded at the size shown.
private struct PackImage: View {
    let path: String
    let width: CGFloat
    @State private var image: CGImage?
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        Group {
            if let image {
                Image(decorative: image, scale: 1)
                    .resizable()
                    .scaledToFill()
            } else {
                ForgeThumbnailPlaceholder(.loading)
            }
        }
        .task(id: path) {
            image = await ForgeImages.thumbnail(
                of: URL(fileURLWithPath: path), maxPixels: ForgeImages.pixels(width, scale: displayScale))
        }
    }
}

// MARK: Home

private struct HomeScreen: View {
    let business: BusinessView
    let act: (String, Bool) -> Void
    @Environment(\.forgeTheme) private var theme
    @ScaledMetric(relativeTo: .largeTitle) private var logoSize: CGFloat = 72

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.l) {
                if let hero = business.heroPath {
                    ForgeThumbnail(aspectRatio: 16 / 9, shape: .rounded) {
                        PackImage(path: hero, width: 400)
                    }
                    .accessibilityHidden(true)
                }
                HStack(alignment: .center, spacing: theme.spacing.m) {
                    if let logo = business.logoPath {
                        ForgeThumbnail(width: logoSize, shape: .rounded, label: Text("\(business.name) logo")) {
                            PackImage(path: logo, width: logoSize)
                        }
                    }
                    VStack(alignment: .leading, spacing: theme.spacing.xxs) {
                        Text(business.name)
                            .font(.largeTitle.bold())
                            .accessibilityAddTraits(.isHeader)
                            .accessibilityIdentifier("preview.name")
                        Text(business.category)
                            .font(.subheadline)
                            .foregroundStyle(theme.colors.textSecondary)
                    }
                }
                if let tagline = business.tagline {
                    Text(tagline)
                        .font(.title3)
                        .forgeForeground(.accent)
                }
                if let book = business.book {
                    BookButton(book: book, act: act)
                }
                if let description = business.description {
                    Text(description)
                        .font(.body)
                        .foregroundStyle(theme.colors.textPrimary)
                }
                quickContacts
            }
            .padding()
        }
        .background(theme.colors.background.ignoresSafeArea())
        .navigationTitle("Home")
        .navigationBarTitleDisplayMode(.inline)
    }

    /// Call and email, one tap from the first screen.
    @ViewBuilder
    private var quickContacts: some View {
        let quick = business.contact.filter { $0.kind == .phone || $0.kind == .email }
        if !quick.isEmpty {
            HStack(spacing: theme.spacing.m) {
                ForEach(quick, id: \.url) { contact in
                    Button {
                        act(contact.url, false)
                    } label: {
                        Label(contact.kind == .phone ? "Call" : "Email",
                              systemImage: contact.kind == .phone ? "phone" : "envelope")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(ForgeButtonStyle(.secondary, expands: true))
                    .accessibilityIdentifier(contact.kind == .phone ? "preview.home.call" : "preview.home.email")
                }
            }
        }
    }
}

/// The primary action: their own booking page, a call or an email.
private struct BookButton: View {
    let book: BookView
    let act: (String, Bool) -> Void

    var body: some View {
        Button {
            act(book.url, book.kind == .page)
        } label: {
            Label(book.label, systemImage: symbol)
                .frame(maxWidth: .infinity)
        }
        .buttonStyle(ForgeButtonStyle(.primary, expands: true))
        .accessibilityIdentifier("preview.book")
        .accessibilityHint(hint)
    }

    private var symbol: String {
        switch book.kind {
        case .page: "calendar"
        case .call: "phone"
        case .email: "envelope"
        }
    }

    private var hint: String {
        switch book.kind {
        case .page: "Opens their booking page."
        case .call: "Calls them."
        case .email: "Writes them an email."
        }
    }
}

// MARK: Services

private struct ServicesScreen: View {
    let business: BusinessView

    var body: some View {
        Group {
            if business.services.isEmpty {
                ForgeStateView.empty(
                    "No \(business.words.servicesTitle) Listed",
                    systemImage: "list.bullet.rectangle",
                    message: "Their website doesn't list them yet."
                )
            } else {
                List {
                    Section {
                        ForEach(Array(business.services.enumerated()), id: \.offset) { index, service in
                            ServiceRow(service: service)
                                .accessibilityIdentifier("preview.service.\(index)")
                        }
                    } footer: {
                        if business.hiddenPrices > 0 {
                            Text("Some prices are shown once the business confirms them.")
                        }
                    }
                }
            }
        }
        .navigationTitle(business.words.servicesTitle)
    }
}

private struct ServiceRow: View {
    let service: ServiceView
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.xxs) {
            HStack(alignment: .firstTextBaseline) {
                Text(service.name)
                    .font(.body.weight(.semibold))
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let price = service.price {
                    Text(price)
                        .font(.body.weight(.semibold))
                        .forgeForeground(.accent)
                }
            }
            if let duration = service.duration {
                Label(duration, systemImage: "clock")
                    .font(.subheadline)
                    .foregroundStyle(theme.colors.textSecondary)
            }
            if let notes = service.notes {
                Text(notes)
                    .font(.subheadline)
                    .foregroundStyle(theme.colors.textSecondary)
            }
        }
        .padding(.vertical, theme.spacing.xxs)
        .accessibilityElement(children: .combine)
    }
}

// MARK: Visit

private struct VisitScreen: View {
    let business: BusinessView
    let act: (String, Bool) -> Void

    var body: some View {
        List {
            if !business.hours.isEmpty {
                Section("Opening Hours") {
                    ForEach(business.hours, id: \.day) { day in
                        LabeledContent(day.day, value: day.times)
                            .accessibilityIdentifier("preview.hours.\(day.day.lowercased())")
                    }
                }
            }
            if !business.contact.isEmpty {
                Section("Contact") {
                    ForEach(Array(business.contact.enumerated()), id: \.offset) { index, contact in
                        Button {
                            act(contact.url, contact.kind == .website)
                        } label: {
                            ForgeRow(contact.label, systemImage: Self.symbol(contact.kind)) {
                                EmptyView()
                            }
                        }
                        .accessibilityHint(Self.hint(contact.kind))
                        .accessibilityIdentifier("preview.contact.\(index)")
                    }
                }
            }
            if let book = business.book {
                Section {
                    BookButton(book: book, act: act)
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                }
            }
        }
        .overlay {
            if business.hours.isEmpty && business.contact.isEmpty && business.book == nil {
                ForgeStateView.empty(
                    "No Visiting Details",
                    systemImage: "mappin.slash",
                    message: "Their website doesn't give an address, hours or a phone number."
                )
            }
        }
        .navigationTitle("Visit")
    }

    private static func symbol(_ kind: ContactKind) -> String {
        switch kind {
        case .phone: "phone"
        case .email: "envelope"
        case .address: "map"
        case .website: "safari"
        }
    }

    private static func hint(_ kind: ContactKind) -> String {
        switch kind {
        case .phone: "Calls them."
        case .email: "Writes them an email."
        case .address: "Shows it in Maps."
        case .website: "Opens the page."
        }
    }
}

// MARK: In-app browser

/// Their own page, in Safari's in-app view: honest about where it is (the
/// address bar shows their site), and nothing pretends to be connected.
private struct InAppBrowser: View {
    let url: URL
    @Environment(\.openURL) private var openURL

    var body: some View {
        #if canImport(SafariServices) && canImport(UIKit)
            SafariView(url: url)
        #else
            Button("Open in Browser") { openURL(url) }
        #endif
    }
}

#if canImport(SafariServices) && canImport(UIKit)
    private struct SafariView: UIViewControllerRepresentable {
        let url: URL

        func makeUIViewController(context: Context) -> SFSafariViewController {
            SFSafariViewController(url: url)
        }

        func updateUIViewController(_ controller: SFSafariViewController, context: Context) {}
    }
#endif
