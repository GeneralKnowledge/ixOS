import SwiftUI

extension View {
    /// Presents `message` in a standard alert with an OK button; dismissing
    /// the alert sets `message` back to nil. Use one per presented screen:
    /// a sheet needs its own, because a view under a sheet cannot present.
    func errorAlert(_ title: LocalizedStringKey, message: Binding<String?>) -> some View {
        alert(
            title,
            isPresented: Binding(
                get: { message.wrappedValue != nil },
                set: { if !$0 { message.wrappedValue = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(message.wrappedValue ?? "")
        }
    }
}
