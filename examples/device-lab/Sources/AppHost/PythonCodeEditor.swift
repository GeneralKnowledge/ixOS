@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

#if canImport(UIKit) && os(iOS)
import UIKit

/// Python Lab's code editor (python-lab-workspace-plan.md, PL1c): a
/// `UITextView` with smart punctuation off, line numbers, Python
/// indentation, a key row above the keyboard, iOS's own find and replace,
/// syntax colours from Rust (`pythonHighlight`, python_highlight.rs) and
/// bracket matching (`pythonMatchingBracket`, PL2).
struct PythonCodeEditor: UIViewRepresentable {
    @Binding var text: String
    var isEditable: Bool
    var accessibilityLabel: String
    /// A line to show and put the cursor at (a traceback's line, PL2b).
    var goTo: PythonLineRequest? = nil
    /// Lines with a breakpoint, marked in the gutter (PL2f).
    var breakpoints: Set<Int> = []
    /// The line Debug paused on, highlighted.
    var pausedLine: Int? = nil
    /// Tapping a line number, or the key row's breakpoint key, toggles a
    /// breakpoint there; nil turns both off.
    var onToggleBreakpoint: ((Int) -> Void)? = nil

    @Environment(\.forgeTheme) private var theme

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    func makeUIView(context: Context) -> CodeTextView {
        // TextKit 1, for the line fragments the gutter draws from.
        let view = CodeTextView(usingTextLayoutManager: false)
        view.delegate = context.coordinator
        context.coordinator.view = view
        view.smartQuotesType = .no
        view.smartDashesType = .no
        view.smartInsertDeleteType = .no
        view.autocorrectionType = .no
        view.spellCheckingType = .no
        view.autocapitalizationType = .none
        view.inlinePredictionType = .no
        view.isFindInteractionEnabled = true
        view.alwaysBounceVertical = true
        view.keyboardDismissMode = .interactive
        view.adjustsFontForContentSizeCategory = true
        view.accessibilityIdentifier = "pythonLab.code"
        view.inputAccessoryView = context.coordinator.keyRow()
        view.onIndent = { [weak coordinator = context.coordinator] outdent in
            coordinator?.indentSelection(outdent: outdent)
        }
        view.registerForTraitChanges([UITraitPreferredContentSizeCategory.self]) {
            [weak coordinator = context.coordinator] (_: CodeTextView, _: UITraitCollection) in
            coordinator?.fontChanged()
        }
        return view
    }

    func updateUIView(_ view: CodeTextView, context: Context) {
        let coordinator = context.coordinator
        coordinator.text = $text
        view.isEditable = isEditable
        view.accessibilityLabel = accessibilityLabel
        coordinator.setTheme(theme)
        coordinator.onToggleBreakpoint = onToggleBreakpoint
        view.onGutterTap = onToggleBreakpoint
        view.breakpointLines = breakpoints
        view.pausedLine = pausedLine
        if view.text != text {
            view.text = text
            view.textChanged()
            coordinator.colourNow()
        }
        if let goTo, goTo.id != coordinator.lastGoTo {
            coordinator.lastGoTo = goTo.id
            let range = PythonLineRequest.range(ofLine: goTo.line, in: view.text as NSString)
            view.selectedRange = NSRange(location: range.location, length: 0)
            view.scrollRangeToVisible(range)
            view.flashScrollIndicators()
        }
    }

    /// The colours from the ForgeUI theme. They follow light and dark mode
    /// by themselves.
    struct EditorColours {
        var plain: UIColor
        var gutter: UIColor
        var keyword: UIColor
        var string: UIColor
        var comment: UIColor
        var number: UIColor
        var definition: UIColor
        var decorator: UIColor
        /// Behind a bracket and its partner (PL2).
        var bracket: UIColor
        /// Behind a bracket with no partner.
        var unmatched: UIColor
        /// A breakpoint's mark in the gutter (PL2f).
        var breakpoint: UIColor
        var onBreakpoint: UIColor
        /// Behind the line Debug paused on.
        var paused: UIColor

        init(theme: ForgeTheme) {
            let colors = theme.colors
            plain = UIColor(colors.textPrimary)
            gutter = UIColor(colors.textTertiary)
            keyword = UIColor(colors.tone(.accent).text)
            string = UIColor(colors.tone(.success).text)
            comment = UIColor(colors.textSecondary)
            number = UIColor(colors.tone(.info).text)
            definition = UIColor(colors.textPrimary)
            decorator = UIColor(colors.tone(.warning).text)
            bracket = UIColor(colors.tone(.accent).soft)
            unmatched = UIColor(colors.tone(.danger).soft)
            breakpoint = UIColor(colors.tone(.danger).solid)
            onBreakpoint = UIColor(colors.tone(.danger).onSolid)
            paused = UIColor(colors.tone(.warning).soft)
        }

        func colour(_ kind: HighlightKind) -> UIColor {
            switch kind {
            case .keyword: keyword
            case .string: string
            case .comment: comment
            case .number: number
            case .definition: definition
            case .decorator: decorator
            }
        }
    }

    @MainActor
    final class Coordinator: NSObject, UITextViewDelegate {
        var text: Binding<String>
        weak var view: CodeTextView?
        /// The last `goTo` done, so it happens once.
        var lastGoTo: UUID?
        var onToggleBreakpoint: ((Int) -> Void)?
        private var colours: EditorColours?
        private var theme: ForgeTheme?
        private var colouring: Task<Void, Never>?
        private var matching: Task<Void, Never>?
        /// A bracket is marked now, to clear on the next move.
        private var marked = false

        init(text: Binding<String>) {
            self.text = text
        }

        static let indent = "    "

        var font: UIFont {
            UIFontMetrics(forTextStyle: .footnote)
                .scaledFont(for: .monospacedSystemFont(ofSize: 13, weight: .regular))
        }

        func setTheme(_ new: ForgeTheme) {
            guard new != theme else { return }
            theme = new
            colours = EditorColours(theme: new)
            fontChanged()
        }

        /// A new theme or text size: everything is coloured again.
        func fontChanged() {
            guard let view, let colours else { return }
            view.gutterColour = colours.gutter
            view.breakpointColour = colours.breakpoint
            view.breakpointTextColour = colours.onBreakpoint
            view.pausedColour = colours.paused
            view.font = font
            view.typingAttributes = [.font: font, .foregroundColor: colours.plain]
            view.textChanged()
            colourNow()
        }

        // MARK: Typing

        func textViewDidChange(_ textView: UITextView) {
            changed()
        }

        func textViewDidChangeSelection(_ textView: UITextView) {
            matchBrackets()
        }

        // MARK: Bracket matching

        /// Marks the bracket beside the cursor and its partner, or the
        /// bracket alone in the danger tone when it has none (PL2). Rust
        /// (`pythonMatchingBracket`) skips strings and comments.
        private func matchBrackets() {
            matching?.cancel()
            guard let view, let colours else { return }
            let selection = view.selectedRange
            let source = view.text ?? ""
            guard selection.length == 0, !source.isEmpty else {
                mark([], colour: colours.bracket)
                return
            }
            matching = Task { [weak self] in
                let found = await Task.detached(priority: .userInitiated) {
                    pythonMatchingBracket(source: source, cursor: UInt32(selection.location))
                }.value
                guard !Task.isCancelled, let self, let view = self.view, view.text == source,
                      view.selectedRange == selection
                else { return }
                guard let found else {
                    self.mark([], colour: colours.bracket)
                    return
                }
                let at = NSRange(location: Int(found.at), length: 1)
                if let partner = found.partner {
                    self.mark([at, NSRange(location: Int(partner), length: 1)], colour: colours.bracket)
                } else {
                    self.mark([at], colour: colours.unmatched)
                }
            }
        }

        /// A background on the text storage, like the colours: an attribute
        /// change never enters Undo, and the file saves only the text.
        /// (TextKit 1's temporary attributes are macOS-only.)
        private func mark(_ ranges: [NSRange], colour: UIColor) {
            guard let view else { return }
            let storage = view.textStorage
            let length = storage.length
            let ranges = ranges.filter { NSMaxRange($0) <= length }
            guard marked || !ranges.isEmpty else { return }
            storage.beginEditing()
            // Typing moves text under the old marks, so clear them all.
            storage.removeAttribute(.backgroundColor, range: NSRange(location: 0, length: length))
            for range in ranges {
                storage.addAttribute(.backgroundColor, value: colour, range: range)
            }
            storage.endEditing()
            marked = !ranges.isEmpty
            // What's typed next never takes a mark's background.
            view.typingAttributes[.backgroundColor] = nil
        }

        private func changed() {
            guard let view else { return }
            view.textChanged()
            let current = view.text ?? ""
            if text.wrappedValue != current {
                text.wrappedValue = current
            }
            colourSoon()
        }

        func textView(
            _ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText replacement: String
        ) -> Bool {
            // Typed beside a marked bracket, text would take its background.
            textView.typingAttributes[.backgroundColor] = nil
            let source = (textView.text ?? "") as NSString
            let lineStart = source.lineRange(for: NSRange(location: range.location, length: 0)).location
            if replacement == "\n" {
                // Keep the line's indentation, and add one level after a `:`
                // (ignoring a comment after it).
                let before = source.substring(with: NSRange(location: lineStart, length: range.location - lineStart))
                var indent = String(before.prefix { $0 == " " || $0 == "\t" })
                let code = before.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false)
                    .first.map(String.init) ?? before
                if code.trimmingCharacters(in: .whitespaces).hasSuffix(":") {
                    indent += Self.indent
                }
                replace(range, with: "\n" + indent)
                return false
            }
            if replacement == "\t" {
                replace(range, with: Self.indent)
                return false
            }
            if replacement.isEmpty, range.length == 1 {
                // Backspace in the indentation removes back to the last tab stop.
                let caret = range.location + 1
                let lead = source.substring(with: NSRange(location: lineStart, length: caret - lineStart))
                if !lead.isEmpty, lead.allSatisfy({ $0 == " " }) {
                    let remove = (lead.count - 1) % Self.indent.count + 1
                    replace(NSRange(location: caret - remove, length: remove), with: "")
                    return false
                }
            }
            return true
        }

        /// Replaces `range` as typing would, so Undo takes it back.
        private func replace(_ range: NSRange, with replacement: String) {
            guard let view,
                  let start = view.position(from: view.beginningOfDocument, offset: range.location),
                  let end = view.position(from: start, offset: range.length),
                  let textRange = view.textRange(from: start, to: end)
            else { return }
            view.replace(textRange, withText: replacement)
            changed()
        }

        func insert(_ snippet: String) {
            guard let view, view.isEditable else { return }
            view.typingAttributes[.backgroundColor] = nil
            if snippet == "\t" {
                indentSelection(outdent: false)
                return
            }
            view.insertText(snippet)
            changed()
        }

        /// Tab and Shift-Tab: with no selection, Tab inserts four spaces;
        /// otherwise every line in the selection moves in or out a level.
        func indentSelection(outdent: Bool) {
            guard let view, view.isEditable else { return }
            let selected = view.selectedRange
            let source = (view.text ?? "") as NSString
            if selected.length == 0 && !outdent {
                replace(selected, with: Self.indent)
                return
            }
            let lines = source.lineRange(for: selected)
            let block = source.substring(with: lines)
            var changedLines: [String] = []
            for line in block.split(separator: "\n", omittingEmptySubsequences: false) {
                if outdent {
                    let spaces = line.prefix { $0 == " " }.count
                    changedLines.append(String(line.dropFirst(min(spaces, Self.indent.count))))
                } else {
                    changedLines.append(line.isEmpty ? "" : Self.indent + line)
                }
            }
            let result = changedLines.joined(separator: "\n")
            guard result != block else { return }
            replace(lines, with: result)
            if selected.length == 0 {
                // Just a caret: it stays where it was in the line's text.
                let removed = (block as NSString).length - (result as NSString).length
                view.selectedRange = NSRange(location: max(lines.location, selected.location - removed), length: 0)
            } else {
                view.selectedRange = NSRange(location: lines.location, length: (result as NSString).length)
            }
        }

        // MARK: Colours

        /// Colours after typing pauses, so a long file doesn't slow typing.
        private func colourSoon() {
            colouring?.cancel()
            colouring = Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(300))
                guard !Task.isCancelled else { return }
                self?.colourNow()
            }
        }

        func colourNow() {
            colouring?.cancel()
            guard let view, let colours else { return }
            let source = view.text ?? ""
            colouring = Task { [weak self] in
                let spans = await Task.detached(priority: .userInitiated) {
                    pythonHighlight(source: source)
                }.value
                guard !Task.isCancelled, let self, let view = self.view, view.text == source else { return }
                Self.apply(spans, to: view, colours: colours, font: self.font)
                // Colouring resets every attribute, the bracket marks too.
                self.marked = false
                self.matchBrackets()
            }
        }

        private static func apply(
            _ spans: [HighlightSpan], to view: UITextView, colours: EditorColours, font: UIFont
        ) {
            let storage = view.textStorage
            let whole = NSRange(location: 0, length: storage.length)
            storage.beginEditing()
            storage.setAttributes([.font: font, .foregroundColor: colours.plain], range: whole)
            for span in spans {
                let range = NSRange(location: Int(span.start), length: Int(span.length))
                guard NSMaxRange(range) <= storage.length else { continue }
                storage.addAttribute(.foregroundColor, value: colours.colour(span.kind), range: range)
                if span.kind == .definition {
                    storage.addAttribute(
                        .font, value: UIFont.monospacedSystemFont(ofSize: font.pointSize, weight: .semibold),
                        range: range)
                }
            }
            storage.endEditing()
        }

        // MARK: Key row

        func keyRow() -> UIView {
            let row = UIInputView(frame: CGRect(x: 0, y: 0, width: 320, height: 44), inputViewStyle: .keyboard)
            row.allowsSelfSizing = true
            let scroll = UIScrollView()
            scroll.showsHorizontalScrollIndicator = false
            scroll.translatesAutoresizingMaskIntoConstraints = false
            let stack = UIStackView()
            stack.axis = .horizontal
            stack.spacing = 6
            stack.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(scroll)
            scroll.addSubview(stack)
            NSLayoutConstraint.activate([
                scroll.leadingAnchor.constraint(equalTo: row.leadingAnchor),
                scroll.trailingAnchor.constraint(equalTo: row.trailingAnchor),
                scroll.topAnchor.constraint(equalTo: row.topAnchor),
                scroll.bottomAnchor.constraint(equalTo: row.bottomAnchor),
                row.heightAnchor.constraint(equalToConstant: 44),
                stack.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor, constant: 8),
                stack.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor, constant: -8),
                stack.centerYAnchor.constraint(equalTo: scroll.frameLayoutGuide.centerYAnchor),
            ])
            stack.addArrangedSubview(key(title: "Tab", label: "Indent", id: "tab") { $0.insert("\t") })
            for (snippet, label, id) in Self.keys {
                stack.addArrangedSubview(key(title: snippet, label: label, id: id) { $0.insert(snippet) })
            }
            stack.addArrangedSubview(key(symbol: "circle.fill", label: "Toggle breakpoint on this line", id: "breakpoint") {
                guard let view = $0.view, let toggle = $0.onToggleBreakpoint else { return }
                toggle(view.line(at: view.selectedRange.location))
            })
            stack.addArrangedSubview(key(symbol: "arrow.uturn.backward", label: "Undo", id: "undo") {
                $0.view?.undoManager?.undo()
                $0.changed()
            })
            stack.addArrangedSubview(key(symbol: "arrow.uturn.forward", label: "Redo", id: "redo") {
                $0.view?.undoManager?.redo()
                $0.changed()
            })
            return row
        }

        private static let keys: [(String, String, String)] = [
            (":", "Colon", "colon"), ("(", "Open parenthesis", "openParen"),
            (")", "Close parenthesis", "closeParen"), ("[", "Open bracket", "openBracket"),
            ("]", "Close bracket", "closeBracket"), ("{", "Open brace", "openBrace"),
            ("}", "Close brace", "closeBrace"), ("\"", "Double quote", "doubleQuote"),
            ("'", "Single quote", "singleQuote"), ("#", "Hash", "hash"), ("=", "Equals", "equals"),
            ("_", "Underscore", "underscore"),
        ]

        private func key(
            title: String? = nil, symbol: String? = nil, label: String, id: String,
            action: @escaping @MainActor (Coordinator) -> Void
        ) -> UIButton {
            var configuration = UIButton.Configuration.gray()
            configuration.title = title
            configuration.image = symbol.flatMap { UIImage(systemName: $0) }
            configuration.contentInsets = NSDirectionalEdgeInsets(top: 6, leading: 12, bottom: 6, trailing: 12)
            configuration.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { attributes in
                var attributes = attributes
                attributes.font = UIFont.monospacedSystemFont(ofSize: 17, weight: .medium)
                return attributes
            }
            let button = UIButton(configuration: configuration, primaryAction: UIAction { [weak self] _ in
                guard let self else { return }
                action(self)
            })
            button.accessibilityLabel = label
            button.accessibilityIdentifier = "pythonLab.key.\(id)"
            return button
        }
    }
}

/// A text view that draws line numbers in its left inset, and takes Tab and
/// Shift-Tab from a hardware keyboard to indent and outdent.
final class CodeTextView: UITextView {
    var gutterColour: UIColor = .tertiaryLabel {
        didSet { setNeedsDisplay() }
    }
    var onIndent: ((_ outdent: Bool) -> Void)?
    /// A tap in the gutter, with the 1-based line beside it (PL2f).
    var onGutterTap: ((Int) -> Void)?
    var breakpointLines: Set<Int> = [] {
        didSet { if breakpointLines != oldValue { setNeedsDisplay() } }
    }
    var pausedLine: Int? {
        didSet { if pausedLine != oldValue { setNeedsDisplay() } }
    }
    var breakpointColour: UIColor = .systemRed {
        didSet { setNeedsDisplay() }
    }
    var breakpointTextColour: UIColor = .white {
        didSet { setNeedsDisplay() }
    }
    var pausedColour: UIColor = .systemYellow.withAlphaComponent(0.25) {
        didSet { setNeedsDisplay() }
    }
    private lazy var gutterTap: UITapGestureRecognizer = {
        let tap = UITapGestureRecognizer(target: self, action: #selector(tappedGutter(_:)))
        return tap
    }()

    /// Where each line starts, in UTF-16 units, for numbering the visible lines.
    private var lineStarts: [Int] = [0]

    override init(frame: CGRect, textContainer: NSTextContainer?) {
        super.init(frame: frame, textContainer: textContainer)
        contentMode = .redraw
        textContainerInset = UIEdgeInsets(top: 8, left: 36, bottom: 8, right: 8)
        addGestureRecognizer(gutterTap)
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        contentMode = .redraw
        addGestureRecognizer(gutterTap)
    }

    /// Only taps on the line numbers are the gutter's; the text keeps the rest.
    override func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
        guard recognizer === gutterTap else { return super.gestureRecognizerShouldBegin(recognizer) }
        return onGutterTap != nil && recognizer.location(in: self).x < textContainerInset.left
    }

    @objc private func tappedGutter(_ recognizer: UITapGestureRecognizer) {
        let point = recognizer.location(in: self)
        let y = point.y - textContainerInset.top
        let extra = layoutManager.extraLineFragmentRect
        if !extra.isEmpty, y >= extra.minY {
            onGutterTap?(lineStarts.count)
            return
        }
        let glyph = layoutManager.glyphIndex(for: CGPoint(x: 0, y: max(0, y)), in: textContainer)
        onGutterTap?(line(at: layoutManager.characterIndexForGlyph(at: glyph)))
    }

    func textChanged() {
        var starts = [0]
        let units = (text ?? "").utf16
        var offset = 0
        for unit in units {
            offset += 1
            if unit == 10 { starts.append(offset) }
        }
        lineStarts = starts
        // Room for the widest line number.
        let digits = String(starts.count).count
        let width = CGFloat(max(digits, 2)) * gutterFont.pointSize * 0.62 + 14
        if abs(textContainerInset.left - width) > 0.5 {
            textContainerInset.left = width
        }
        setNeedsDisplay()
    }

    private var gutterFont: UIFont {
        UIFontMetrics(forTextStyle: .caption2)
            .scaledFont(for: .monospacedDigitSystemFont(ofSize: 11, weight: .regular))
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        // Scrolling moves the bounds, so the visible numbers change.
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        super.draw(rect)
        let manager = layoutManager
        let inset = textContainerInset
        let attributes: [NSAttributedString.Key: Any] = [.font: gutterFont, .foregroundColor: gutterColour]
        let width = inset.left - 8
        func number(_ line: Int, atY y: CGFloat, height: CGFloat) {
            let label = "\(line)" as NSString
            if line == pausedLine {
                // Behind the text, which draws in a view above this one.
                pausedColour.setFill()
                UIRectFill(CGRect(x: 0, y: y + inset.top, width: bounds.width, height: height))
            }
            var numberAttributes = attributes
            if breakpointLines.contains(line) {
                let mark = CGRect(x: 2, y: y + inset.top + 1, width: width + 2, height: height - 2)
                breakpointColour.setFill()
                UIBezierPath(roundedRect: mark, cornerRadius: min(6, mark.height / 2)).fill()
                numberAttributes[.foregroundColor] = breakpointTextColour
            }
            let size = label.size(withAttributes: numberAttributes)
            label.draw(
                at: CGPoint(x: width - size.width, y: y + inset.top + (height - size.height) / 2),
                withAttributes: numberAttributes)
        }
        let visible = rect.offsetBy(dx: -inset.left, dy: -inset.top)
        let glyphs = manager.glyphRange(forBoundingRect: visible, in: textContainer)
        let source = (text ?? "") as NSString
        manager.enumerateLineFragments(forGlyphRange: glyphs) { fragment, _, _, range, _ in
            let start = manager.characterIndexForGlyph(at: range.location)
            guard start == 0 || source.character(at: start - 1) == 10 else { return }
            number(self.line(at: start), atY: fragment.minY, height: fragment.height)
        }
        let extra = manager.extraLineFragmentRect
        if !extra.isEmpty, extra.intersects(visible) {
            number(lineStarts.count, atY: extra.minY, height: extra.height)
        }
    }

    /// The 1-based line holding UTF-16 offset `offset`.
    func line(at offset: Int) -> Int {
        var low = 0
        var high = lineStarts.count
        while low + 1 < high {
            let mid = (low + high) / 2
            if lineStarts[mid] <= offset { low = mid } else { high = mid }
        }
        return low + 1
    }

    override var keyCommands: [UIKeyCommand]? {
        let tab = UIKeyCommand(input: "\t", modifierFlags: [], action: #selector(indentKey))
        tab.wantsPriorityOverSystemBehavior = true
        let backTab = UIKeyCommand(input: "\t", modifierFlags: .shift, action: #selector(outdentKey))
        backTab.wantsPriorityOverSystemBehavior = true
        backTab.discoverabilityTitle = "Outdent"
        return (super.keyCommands ?? []) + [tab, backTab]
    }

    @objc private func indentKey() { onIndent?(false) }
    @objc private func outdentKey() { onIndent?(true) }
}
#else
/// Elsewhere (macOS builds of the package) a plain editor.
struct PythonCodeEditor: View {
    @Binding var text: String
    var isEditable: Bool
    var accessibilityLabel: String
    /// A line to show and put the cursor at (a traceback's line, PL2b).
    var goTo: PythonLineRequest? = nil
    var breakpoints: Set<Int> = []
    var pausedLine: Int? = nil
    var onToggleBreakpoint: ((Int) -> Void)? = nil

    var body: some View {
        TextEditor(text: $text)
            .font(.system(.footnote, design: .monospaced))
            .disabled(!isEditable)
            .accessibilityLabel(accessibilityLabel)
            .accessibilityIdentifier("pythonLab.code")
    }
}
#endif
