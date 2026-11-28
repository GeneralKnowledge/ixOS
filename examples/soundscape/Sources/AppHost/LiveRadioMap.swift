#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeUI
import MapKit
import SwiftUI

// Radio Map (docs/podcasts-and-radio.md, "Radio Map"): live stations at the
// places Radio Browser records for them, on Apple's map. Zoomed out it is
// the globe. Which stations, and whether they are inside the area asked
// about, is Rust's (`LiveAsk::Mapped`, `LiveAsk::Near`); this only draws.

/// Live stations on the map: the most played worldwide at first, then the
/// most played around wherever the map is moved to. Tap a station to play it.
struct LiveRadioMap: View {
    @Environment(LivePlayer.self) private var player
    @Environment(LiveRadioModel.self) private var live

    @State private var position: MapCameraPosition = .camera(
        MapCamera(centerCoordinate: CLLocationCoordinate2D(latitude: 30, longitude: 10), distance: 25_000_000)
    )
    @State private var ask: LiveAsk = .mapped
    @State private var stations: [LiveStation] = []
    @State private var selected: String?
    @State private var problem: String?
    @State private var loading = false
    @State private var attempt = 0
    /// Which request owns `loading` (a superseded one mustn't clear it).
    @State private var request = 0
    /// The area in view once the map stops moving, when it is small enough
    /// to ask about on its own.
    @State private var area: LiveAsk?

    var body: some View {
        Map(position: $position, selection: $selected) {
            ForEach(stations) { station in
                if let at = station.location {
                    Marker(
                        station.name, systemImage: "dot.radiowaves.left.and.right",
                        coordinate: CLLocationCoordinate2D(latitude: at.latitude, longitude: at.longitude)
                    )
                    .tint(player.station?.uuid == station.uuid ? .pink : .purple)
                    .tag(station.uuid)
                }
            }
        }
        .mapStyle(.hybrid(elevation: .realistic, pointsOfInterest: .excludingAll))
        .mapControls {
            MapCompass()
            MapPitchToggle()
        }
        .onMapCameraChange(frequency: .onEnd) { context in
            area = Self.area(context.region)
        }
        .onChange(of: selected) { _, uuid in
            guard let uuid, let station = stations.first(where: { $0.uuid == uuid }) else { return }
            // Already on: leave it. Paused or failed: tapping tries again.
            let on = player.station?.uuid == uuid && (player.isPlaying || player.isConnecting)
            if on { return }
            Task { await player.tune(station) }
        }
        .safeAreaInset(edge: .top) { searchHere }
        .safeAreaInset(edge: .bottom) { selectedCard }
        .overlay {
            if let problem, stations.isEmpty {
                ForgeStateView.failed(
                    "Can't Reach Radio Browser", message: problem,
                    retry: ForgeStateAction("Try Again", identifier: "live.map.retry") { attempt += 1 }
                )
            } else if loading && stations.isEmpty {
                ForgeStateView.loading("Finding stations")
            }
        }
        .navigationTitle("Radio Map")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                NavigationLink(value: ListenRoute.liveMapList(ask, stations)) {
                    Label("Show as List", systemImage: "list.bullet")
                }
                .accessibilityIdentifier("live.map.list")
            }
        }
        .accessibilityIdentifier("live.map")
        .task(id: attempt) { await find() }
    }

    /// Asks about the area in view, once the map has moved somewhere new;
    /// back to the worldwide stations once it shows the globe again; and
    /// why the last ask failed while earlier pins stay.
    @ViewBuilder private var searchHere: some View {
        if let problem, !stations.isEmpty, !loading {
            Button {
                attempt += 1
            } label: {
                Label("\(problem) Try Again", systemImage: "exclamationmark.triangle")
                    .font(.subheadline)
                    .lineLimit(2)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
            }
            .buttonStyle(.bordered)
            .buttonBorderShape(.capsule)
            .background(.regularMaterial, in: Capsule())
            .accessibilityIdentifier("live.map.retry")
            .padding(.top, 8)
        } else if let area, area != ask {
            pill("Stations Here", systemImage: "magnifyingglass",
                 hint: "Shows the most played stations in this part of the map",
                 identifier: "live.map.searchHere") { ask = area }
        } else if area == nil, ask != .mapped, !loading {
            pill("Stations Worldwide", systemImage: "globe",
                 hint: "Shows the most played stations around the world",
                 identifier: "live.map.worldwide") { ask = .mapped }
        } else if loading, !stations.isEmpty {
            ProgressView()
                .padding(8)
                .background(.regularMaterial, in: Capsule())
                .padding(.top, 8)
        }
    }

    private func pill(
        _ title: LocalizedStringKey, systemImage: String, hint: LocalizedStringKey, identifier: String,
        ask change: @escaping () -> Void
    ) -> some View {
        Button {
            change()
            attempt += 1
        } label: {
            Label(title, systemImage: systemImage)
                .font(.subheadline.weight(.semibold))
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
        }
        .buttonStyle(.borderedProminent)
        .buttonBorderShape(.capsule)
        .accessibilityHint(hint)
        .accessibilityIdentifier(identifier)
        .padding(.top, 8)
    }

    /// The tapped station: what it is, and Save.
    @ViewBuilder private var selectedCard: some View {
        if let uuid = selected, let station = stations.first(where: { $0.uuid == uuid }) {
            HStack(spacing: 12) {
                Artwork(title: station.name, url: station.artworkUrl, size: 44)
                VStack(alignment: .leading, spacing: 2) {
                    Text(station.name).font(.headline).lineLimit(1)
                    Text(station.summary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                Button {
                    Task { await live.toggleSaved(station) }
                } label: {
                    Image(systemName: live.isSaved(station.uuid) ? "bookmark.fill" : "bookmark")
                        .font(.title3)
                }
                .accessibilityLabel(live.isSaved(station.uuid) ? "Remove from Your Stations" : "Save to Your Stations")
                .accessibilityIdentifier("live.map.save")
                Button {
                    selected = nil
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                }
                .accessibilityLabel("Close")
                .accessibilityIdentifier("live.map.close")
            }
            .padding(12)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16))
            .padding(.horizontal)
            .padding(.bottom, 8)
            .contextMenu { LiveStationMenu(station: station) }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("live.map.selected")
        }
    }

    private func find() async {
        request += 1
        let mine = request
        loading = true
        defer { if request == mine { loading = false } }
        problem = nil
        let result = await live.stations(ask)
        // A newer ask replaced this one: its answer is out of date.
        guard !Task.isCancelled, request == mine else { return }
        switch result {
        case .success(let found):
            stations = found
            if let selected, !found.contains(where: { $0.uuid == selected }) { self.selected = nil }
        case .failure(let failure):
            if !failure.stopped { problem = failure.message }
        }
    }

    /// The area in view as a question for Radio Browser, or nil while most
    /// of the globe is showing (the worldwide stations already cover it).
    /// Rust keeps the radius within its limits.
    static func area(_ region: MKCoordinateRegion) -> LiveAsk? {
        let span = region.span
        guard span.latitudeDelta < 60 else { return nil }
        // Half the shorter side of the view, a degree of latitude being
        // about 111 km: the circle fits inside what is on screen.
        let latitudeKm = span.latitudeDelta * 111
        let longitudeKm = span.longitudeDelta * 111 * cos(region.center.latitude * .pi / 180)
        let radiusKm = max(min(latitudeKm, longitudeKm) / 2, 1)
        // Round, so a nudge of the map doesn't count as a new area.
        func round(_ value: Double, to step: Double) -> Double { (value / step).rounded() * step }
        let step = max(radiusKm / 111 / 4, 0.01)
        // Rounding can step just past the poles or the date line.
        let latitude = min(max(round(region.center.latitude, to: step), -90), 90)
        var longitude = round(region.center.longitude, to: step).truncatingRemainder(dividingBy: 360)
        if longitude > 180 { longitude -= 360 } else if longitude < -180 { longitude += 360 }
        return .near(
            latitude: latitude,
            longitude: longitude,
            radiusKm: radiusKm.rounded()
        )
    }
}
#endif
