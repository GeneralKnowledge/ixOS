import SwiftUI

/// What iPhone keeps from apps, and why Device Lab does not show it. The
/// short version: some needs a paid developer team, some needs Apple's
/// approval, and some no app may read at all.
struct LimitsView: View {
    private struct Limit: Identifiable {
        let title: String
        let symbol: String
        let detail: String
        var id: String { title }
    }

    private let paid = [
        Limit(
            title: "Wi-Fi Network Name", symbol: "wifi",
            detail: "Needs the Access Wi-Fi Information entitlement, which a free Apple account cannot sign, and location permission on top. Device Lab shows whether you are on Wi-Fi, not which network."),
    ]

    private let approval = [
        Limit(
            title: "Your Phone's Name", symbol: "iphone",
            detail: "Since iOS 16 apps see the model (\"iPhone\"). The real name needs an entitlement Apple grants on request."),
    ]

    private let never = [
        Limit(
            title: "Carrier and Signal Strength", symbol: "antenna.radiowaves.left.and.right",
            detail: "Apple retired the carrier API (it returns placeholders) and has never offered signal strength to apps."),
        Limit(
            title: "Ambient Light Sensor", symbol: "sun.max",
            detail: "Used by iOS for auto-brightness only. Apps see the screen brightness it sets."),
        Limit(
            title: "MAC and IP Addresses", symbol: "number",
            detail: "Hardware addresses are hidden from apps, and Wi-Fi addresses are private per network."),
        Limit(
            title: "Nearby Bluetooth Devices", symbol: "dot.radiowaves.left.and.right",
            detail: "Possible for Bluetooth Low Energy devices that advertise, after asking. Not built yet: Forge's Bluetooth service reports on and off only."),
        Limit(
            title: "Sensors in the Background", symbol: "moon.zzz",
            detail: "Device Lab stops every sensor when you leave it. Background location is possible but needs the \"Always\" permission."),
    ]

    var body: some View {
        List {
            section("Needs a Paid Developer Team", paid)
            section("Needs Apple's Approval", approval)
            section("Not Available to Apps", never)
        }
        .groupedList()
        .navigationTitle("What iOS Keeps")
        .inlineNavigationTitle()
    }

    private func section(_ title: String, _ limits: [Limit]) -> some View {
        Section(title) {
            ForEach(limits) { limit in
                VStack(alignment: .leading, spacing: 4) {
                    Label(limit.title, systemImage: limit.symbol)
                    Text(limit.detail)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 2)
            }
        }
    }
}
