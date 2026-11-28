#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeUI
import MapKit
import SwiftUI

// Radio Map (docs/podcasts-and-radio.md, "Radio Map"): live stations at the
// places Radio Browser records for them, on Apple's map. Zoomed out it is
// the globe. Which stations, and whether they are inside the area asked
// about, is Rust's (`LiveAsk::Mapped`, `LiveAsk::Near`); this only draws.

/// Stations close together on screen, drawn as one pin: the most played
/// one's logo and how many there are. Tapping it lists them.
struct LiveStationGroup: Identifiable {
    /// The most played station's UUID.
    let id: String
    let coordinate: CLLocationCoordinate2D
    /// Most played first.
    let stations: [LiveStation]

    /// Groups `stations` (most played first) into cells about a fourteenth
    /// of the view's height across, so pins never hide each other; stations
    /// at the very same place always share one.
    static func make(_ stations: [LiveStation], span latitudeDelta: Double) -> [LiveStationGroup] {
        let cell = max(latitudeDelta / 14, 0.0005)
        var order: [String] = []
        var members: [String: [LiveStation]] = [:]
        for station in stations {
            guard let at = station.location else { continue }
            let key = "\(Int((at.latitude / cell).rounded(.down))):\(Int((at.longitude / cell).rounded(.down)))"
            if members[key] == nil { order.append(key) }
            members[key, default: []].append(station)
        }
        return order.compactMap { key in
            guard let group = members[key], let first = group.first, let at = first.location else { return nil }
            return LiveStationGroup(
                id: first.uuid,
                coordinate: CLLocationCoordinate2D(latitude: at.latitude, longitude: at.longitude),
                stations: group
            )
        }
    }
}

/// Live stations on the map: the most played worldwide at first, then the
/// most played around wherever the map is moved to. Tap a station to play it;
/// tap a group to list its stations.
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
    /// Whether `stations` is the answer to `ask` (not the empty start, or
    /// what an earlier ask found before this one failed).
    @State private var answered = false
    /// The area in view once the map stops moving, when it is small enough
    /// to ask about on its own.
    @State private var area: LiveAsk?
    /// How much of the globe is in view (degrees of latitude): how close
    /// pins may be before they are grouped.
    @State private var span: Double = 120
    /// The group whose stations are listed.
    @State private var opened: LiveStationGroup?

    var body: some View {
        Map(position: $position) {
            ForEach(LiveStationGroup.make(stations, span: span)) { group in
                Annotation(group.stations.count == 1 ? group.stations[0].name : "", coordinate: group.coordinate) {
                    pin(group)
                }
            }
        }
        .mapStyle(.hybrid(elevation: .realistic, pointsOfInterest: .excludingAll))
        .annotationTitles(.hidden)
        .mapControls {
            MapCompass()
            MapPitchToggle()
        }
        .onMapCameraChange(frequency: .onEnd) { context in
            area = Self.area(context.region)
            span = context.region.span.latitudeDelta
        }
        .sheet(item: $opened) { group in
            NavigationStack {
                List(group.stations) { LiveStationRow(station: $0) }
                    .navigationTitle("\(group.stations.count) Stations")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .confirmationAction) {
                            Button("Done") { opened = nil }
                                .accessibilityIdentifier("live.map.group.done")
                        }
                    }
            }
            .presentationDetents([.medium, .large])
            .environment(player)
            .environment(live)
            .accessibilityIdentifier("live.map.group")
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
                NavigationLink(value: answered
                    ? ListenRoute.liveMapList(ask, stations)
                    : ListenRoute.liveStations(ask, title: "On the Map")) {
                    Label("Show as List", systemImage: "list.bullet")
                }
                .accessibilityIdentifier("live.map.list")
            }
        }
        .accessibilityIdentifier("live.map")
        .task(id: attempt) { await find() }
    }

    /// One pin: the station's logo (or the most played one's, with how many
    /// are there), ringed while it plays.
    private func pin(_ group: LiveStationGroup) -> some View {
        let top = group.stations[0]
        let count = group.stations.count
        let playing = group.stations.contains { $0.uuid == player.station?.uuid }
        return Button {
            if count == 1 { play(top) } else { opened = group }
        } label: {
            Artwork(title: top.name, url: top.artworkUrl, size: 34, circle: true)
                .overlay(Circle().strokeBorder(playing ? Color.pink : Color.white, lineWidth: playing ? 3 : 2))
                .shadow(radius: 2)
                .overlay(alignment: .topTrailing) {
                    if count > 1 {
                        Text(count > 99 ? "99+" : "\(count)")
                            .font(.caption2.weight(.bold))
                            .monospacedDigit()
                            .foregroundStyle(.white)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(.purple, in: Capsule())
                            .offset(x: 8, y: -6)
                    }
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(count == 1 ? top.name : "\(count) stations, including \(top.name)")
        .accessibilityHint(count == 1 ? "Plays the station live" : "Lists the stations here")
        .accessibilityIdentifier(count == 1 ? "live.map.pin" : "live.map.groupPin")
    }

    /// Plays `station` and shows its card. Already on: leave it; paused or
    /// failed: tapping tries again.
    private func play(_ station: LiveStation) {
        selected = station.uuid
        let on = player.station?.uuid == station.uuid && (player.isPlaying || player.isConnecting)
        if on { return }
        Task { await player.tune(station) }
    }

    /// Asks about the area in view, once the map has moved somewhere new;
    /// back to the worldwide stations once it shows the globe again; and
    /// why the last ask failed while earlier pins stay.
    private var searchHere: some View {
        VStack(spacing: 6) {
            failedHere
            moveHere
        }
    }

    /// Why the last ask failed, with Try Again, while earlier pins stay.
    @ViewBuilder private var failedHere: some View {
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
        }
    }

    /// Stations Here, Stations Worldwide, or that an ask is under way.
    @ViewBuilder private var moveHere: some View {
        if let area, area != ask {
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
            // The list mustn't pair the new ask with the old stations.
            answered = false
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
        answered = false
        let result = await live.stations(ask)
        // A newer ask replaced this one: its answer is out of date.
        guard !Task.isCancelled, request == mine else { return }
        switch result {
        case .success(let found):
            answered = true
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
