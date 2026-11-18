#if canImport(SwiftUI)
import SwiftUI

extension View {
    /// Keeps the sheet this view is in from being closed by a swipe down
    /// while `isActive`: while it saves, or while it holds something the
    /// person would lose. Buttons that close the sheet still work.
    ///
    ///     Form { ... }
    ///         .forgeSwipeGuard(!draft.isEmpty)
    ///
    /// Use it instead of SwiftUI's `interactiveDismissDisabled`. On the iPhone
    /// (iOS 26.3, 2026-11-18) that modifier didn't stop the swipe when applied
    /// inside the sheet's `NavigationStack`, which is where a form's modifiers
    /// sit: the showcase's task form and Pocket Library's Add Book both
    /// closed with text typed. This also tells the sheet's own controller,
    /// which works wherever in the sheet it is applied. `forgeFormToolbar`
    /// uses it.
    public func forgeSwipeGuard(_ isActive: Bool = true) -> some View {
        interactiveDismissDisabled(isActive)
        #if canImport(UIKit)
            .background(ForgeSwipeGuard(blocks: isActive).frame(width: 0, height: 0))
        #endif
    }
}

#if canImport(UIKit)
import UIKit

/// Sets `isModalInPresentation` on the controller the sheet presents, so a
/// swipe down can't close it while `blocks`. Found by walking up from this
/// view's controller to the top of its parent chain, which is the presented
/// one; outside a sheet that is the window's root, where the flag does nothing.
private struct ForgeSwipeGuard: UIViewControllerRepresentable {
    let blocks: Bool

    func makeUIViewController(context: Context) -> Controller { Controller() }

    func updateUIViewController(_ controller: Controller, context: Context) {
        controller.blocks = blocks
    }

    final class Controller: UIViewController {
        var blocks = false {
            didSet { apply() }
        }

        override func viewDidLoad() {
            super.viewDidLoad()
            view.isUserInteractionEnabled = false
            view.isAccessibilityElement = false
            view.backgroundColor = .clear
        }

        override func didMove(toParent parent: UIViewController?) {
            super.didMove(toParent: parent)
            apply()
        }

        override func viewWillAppear(_ animated: Bool) {
            super.viewWillAppear(animated)
            apply()
        }

        /// Leaving the screen (a pushed screen popped inside a sheet) lets
        /// the sheet be swiped away again.
        override func willMove(toParent parent: UIViewController?) {
            if parent == nil, blocks { presented?.isModalInPresentation = false }
            super.willMove(toParent: parent)
        }

        private var presented: UIViewController? {
            guard parent != nil else { return nil }
            var top: UIViewController = self
            while let next = top.parent { top = next }
            return top
        }

        private func apply() {
            presented?.isModalInPresentation = blocks
        }
    }
}
#endif
#endif
