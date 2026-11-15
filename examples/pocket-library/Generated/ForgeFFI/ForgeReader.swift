// Copied into an app's Generated/ForgeFFI by scripts/generate-bindings.sh
// when its bindings hold the reader's types (`forge_reader::reader_types!()`
// in the app's Rust). Do not edit the copy; the source of truth is
// crates/forge-reader/swift/ForgeReader.swift.
//
// ForgeReader's SwiftUI half (docs/forge-reader.md): it lays out a
// ReaderSection's blocks natively, so text follows Dynamic Type, dark mode
// and the reader's own text settings, and reports which block is at the
// top of the screen. Rust owns the document and what a position means;
// these views only draw blocks and report places. Empty where SwiftUI is
// missing (Linux, where `swift test` runs an app's models).
#if canImport(SwiftUI)
import Foundation
import SwiftUI

// MARK: - Settings

/// The reader's text settings, shared by every reader in the app: steps
/// larger or smaller than the system text size, and serif or system font.
public enum ForgeReaderSettings {
    public static let textStepKey = "forgeReader.textStep"
    public static let serifKey = "forgeReader.serif"

    /// The text size `step` steps from the system's `base`, within the
    /// sizes iOS has.
    public static func textSize(base: DynamicTypeSize, step: Int) -> DynamicTypeSize {
        let sizes = DynamicTypeSize.allCases
        let index = sizes.firstIndex(of: base) ?? sizes.startIndex
        return sizes[min(max(index + step, 0), sizes.count - 1)]
    }
}

/// Larger, smaller and system text, and the font: the reader's toolbar
/// menu. `identifier` names it for UI tests (`reader.textSettings`).
public struct ForgeReaderTextMenu: View {
    @AppStorage(ForgeReaderSettings.textStepKey) private var textStep = 0
    @AppStorage(ForgeReaderSettings.serifKey) private var serif = true
    @Environment(\.dynamicTypeSize) private var systemTextSize
    private let identifier: String

    public init(identifier: String) {
        self.identifier = identifier
    }

    public var body: some View {
        let size = ForgeReaderSettings.textSize(base: systemTextSize, step: textStep)
        Menu {
            Button("Larger Text", systemImage: "textformat.size.larger") { textStep += 1 }
                .disabled(size == DynamicTypeSize.allCases.last)
            Button("Smaller Text", systemImage: "textformat.size.smaller") { textStep -= 1 }
                .disabled(size == DynamicTypeSize.allCases.first)
            if textStep != 0 {
                Button("System Text Size", systemImage: "arrow.counterclockwise") { textStep = 0 }
            }
            Picker("Font", selection: $serif) {
                Text("Serif").tag(true)
                Text("System").tag(false)
            }
        } label: {
            Label("Text Settings", systemImage: "textformat.size")
        }
        .accessibilityIdentifier(identifier)
    }
}

// MARK: - A section

/// One section (a chapter, an article) as a scrolling column of blocks,
/// with the app's own views above and below it.
///
/// - `topBlock` is kept at the index of the block at the top of the screen
///   (`blocks.count` for the footer), for the app to save as an anchor.
/// - Setting `scrollTarget` scrolls to that block once its row exists
///   (restoring a saved place, or following read-aloud), then sets it back
///   to nil and `topBlock` to the target.
/// - `spokenBlock` and `spokenWord` highlight the word being read aloud.
public struct ForgeReaderSectionView<Header: View, Footer: View>: View {
    private let blocks: [ReaderBlock]
    private let spokenBlock: Int?
    private let spokenWord: NSRange?
    @Binding private var topBlock: Int?
    @Binding private var scrollTarget: Int?
    private let header: Header
    private let footer: Footer

    @AppStorage(ForgeReaderSettings.textStepKey) private var textStep = 0
    @AppStorage(ForgeReaderSettings.serifKey) private var serif = true
    @Environment(\.dynamicTypeSize) private var systemTextSize

    public init(
        blocks: [ReaderBlock],
        topBlock: Binding<Int?>,
        scrollTarget: Binding<Int?> = .constant(nil),
        spokenBlock: Int? = nil,
        spokenWord: NSRange? = nil,
        @ViewBuilder header: () -> Header,
        @ViewBuilder footer: () -> Footer
    ) {
        self.blocks = blocks
        self._topBlock = topBlock
        self._scrollTarget = scrollTarget
        self.spokenBlock = spokenBlock
        self.spokenWord = spokenWord
        self.header = header()
        self.footer = footer()
    }

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    header
                    ForEach(Array(blocks.enumerated()), id: \.offset) { index, block in
                        ForgeReaderBlockView(
                            block: block,
                            serif: serif,
                            spokenWord: spokenBlock == index ? spokenWord : nil
                        )
                        .id(index)
                    }
                    footer
                        .id(blocks.count)
                }
                .scrollTargetLayout()
                .padding()
                .frame(maxWidth: 640)
                .frame(maxWidth: .infinity)
                .dynamicTypeSize(ForgeReaderSettings.textSize(base: systemTextSize, step: textStep))
            }
            .scrollPosition(id: $topBlock, anchor: .top)
            .task(id: scrollTarget) {
                guard let target = scrollTarget else { return }
                if target > 0 {
                    // Rows of a newly loaded section exist a moment later.
                    let showing = topBlock != nil
                    if !showing { try? await Task.sleep(for: .milliseconds(50)) }
                    withAnimation(showing ? .default : nil) {
                        proxy.scrollTo(target, anchor: .top)
                    }
                }
                topBlock = target
                scrollTarget = nil
            }
        }
    }
}

extension ForgeReaderSectionView where Header == EmptyView, Footer == EmptyView {
    public init(blocks: [ReaderBlock], topBlock: Binding<Int?>) {
        self.init(blocks: blocks, topBlock: topBlock, header: { EmptyView() }, footer: { EmptyView() })
    }
}

// MARK: - A block

/// One paragraph, heading, quote, list item, scene break or picture.
/// Links in the text open through the environment's `openURL`, so an app
/// decides where they go (a web sheet, Safari).
public struct ForgeReaderBlockView: View {
    public let block: ReaderBlock
    public let serif: Bool
    /// The word being read aloud, if this block is being read.
    public var spokenWord: NSRange?

    public init(block: ReaderBlock, serif: Bool, spokenWord: NSRange? = nil) {
        self.block = block
        self.serif = serif
        self.spokenWord = spokenWord
    }

    private var design: Font.Design { serif ? .serif : .default }

    public var body: some View {
        switch block.kind {
        case .heading(let level):
            Text(attributed)
                .font(.system(Self.headingStyle(level), design: design, weight: .semibold))
                .padding(.top, level <= 2 ? 16 : 8)
                .accessibilityAddTraits(.isHeader)
        case .paragraph:
            Text(attributed)
                .font(.system(.body, design: design))
                .lineSpacing(4)
        case .quote:
            Text(attributed)
                .font(.system(.body, design: design))
                .lineSpacing(4)
                .foregroundStyle(.secondary)
                .padding(.leading, 16)
                .overlay(alignment: .leading) {
                    Capsule()
                        .fill(.quaternary)
                        .frame(width: 3)
                }
        case .listItem(let marker):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(marker)
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                Text(attributed)
                    .lineSpacing(4)
            }
            .font(.system(.body, design: design))
        case .preformatted:
            ScrollView(.horizontal) {
                Text(attributed)
                    .font(.system(.callout, design: .monospaced))
            }
        case .separator:
            Text("⁂")
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity)
                .accessibilityLabel("Section break")
        case .image(let source, let alt):
            ForgeReaderImage(source: source, alt: alt)
        }
    }

    private static func headingStyle(_ level: UInt8) -> Font.TextStyle {
        switch level {
        case 1: return .title
        case 2: return .title2
        case 3: return .title3
        default: return .headline
        }
    }

    /// The block's spans as styled text: what read-aloud speaks is exactly
    /// this text, so word ranges line up with it.
    public static func text(of block: ReaderBlock) -> String {
        block.spans.map(\.text).joined()
    }

    private var attributed: AttributedString {
        var text = AttributedString()
        for span in block.spans {
            var part = AttributedString(span.text)
            var intent: InlinePresentationIntent = []
            if span.bold { intent.insert(.stronglyEmphasized) }
            if span.italic { intent.insert(.emphasized) }
            if span.monospace { intent.insert(.code) }
            if !intent.isEmpty { part.inlinePresentationIntent = intent }
            if let link = span.link.flatMap(URL.init(string:)) {
                part.link = link
            }
            text.append(part)
        }
        if let spokenWord, let range = Range<AttributedString.Index>(spokenWord, in: text) {
            text[range].backgroundColor = Color.accentColor.opacity(0.3)
        }
        return text
    }
}

/// A picture, scaled to the text column: a saved file, or one on the web.
public struct ForgeReaderImage: View {
    public let source: String
    public let alt: String
    @State private var image: Image?
    @State private var loaded = false

    public init(source: String, alt: String) {
        self.source = source
        self.alt = alt
    }

    private var webURL: URL? {
        guard let url = URL(string: source), let scheme = url.scheme?.lowercased(),
              scheme == "https" || scheme == "http" else { return nil }
        return url
    }

    public var body: some View {
        Group {
            if let webURL {
                AsyncImage(url: webURL) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFit().frame(maxWidth: .infinity)
                    case .failure:
                        EmptyView()
                    default:
                        ProgressView().frame(maxWidth: .infinity, minHeight: 120)
                    }
                }
            } else if let image {
                image
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity)
            } else if !loaded {
                ProgressView()
                    .frame(maxWidth: .infinity, minHeight: 120)
            }
        }
        .accessibilityElement()
        .accessibilityLabel(alt.isEmpty ? "Picture" : alt)
        .accessibilityHidden(alt.isEmpty)
        .task(id: source) {
            guard webURL == nil else { return }
            #if canImport(ImageIO)
                if let cg = await ForgeImages.thumbnail(of: URL(fileURLWithPath: source), maxPixels: 1600) {
                    image = Image(decorative: cg, scale: 1)
                }
            #endif
            loaded = true
        }
    }
}
#endif
