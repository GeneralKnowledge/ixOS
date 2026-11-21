import SwiftUI

/// The places list: a card per page over its own sky, search to add a city,
/// swipe to delete and Edit to reorder. Tapping a card shows its page.
struct LocationsView: View {
    @Bindable var model: WeatherViewModel
    @Binding var isPresented: Bool
    @State private var query = ""

    var body: some View {
        NavigationStack {
            List {
                if query.trimmingCharacters(in: .whitespaces).isEmpty {
                    if let problem = model.placesProblem {
                        SavedPlacesProblemRow(problem: problem) { model.startNewSavedPlaces() }
                            .listRowSeparator(.hidden)
                    }
                    places
                    NavigationLink("Units") { UnitsView(model: model) }
                        .accessibilityIdentifier("places.units")
                        .listRowSeparator(.hidden)
                } else {
                    results
                }
            }
            .listStyle(.plain)
            .navigationTitle("Weather")
            .searchable(text: $query, prompt: "Search for a city")
            .onSubmit(of: .search) { model.search(query) }
            .onChange(of: query) { _, newValue in
                // Below three letters the old results don't match any more.
                if newValue.trimmingCharacters(in: .whitespaces).count >= 3 {
                    model.search(newValue)
                } else {
                    model.clearSearch()
                }
            }
            .toolbar {
                #if os(iOS)
                    ToolbarItem(placement: .topBarLeading) { EditButton().accessibilityIdentifier("places.edit") }
                #endif
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { isPresented = false }
                        .accessibilityIdentifier("places.done")
                }
            }
        }
        .alert(
            "Couldn't Change Your Places",
            isPresented: Binding(
                get: { model.placesError != nil },
                set: { if !$0 { model.placesError = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
                .accessibilityIdentifier("places.errorOK")
        } message: {
            Text(model.placesError ?? "")
        }
    }

    @ViewBuilder private var places: some View {
        ForEach(model.pages) { page in
            Button {
                model.selection = page.id
                isPresented = false
            } label: {
                PlaceCard(page: page)
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier(page.isMyLocation ? "places.card.here" : "places.card.\(page.place?.name ?? "place")")
            .listRowSeparator(.hidden)
            .listRowInsets(EdgeInsets(top: 5, leading: 16, bottom: 5, trailing: 16))
            .deleteDisabled(page.isMyLocation)
            .moveDisabled(page.isMyLocation)
        }
        .onDelete { offsets in
            model.remove(offsets.map { model.pages[$0].id })
        }
        .onMove { source, destination in
            // Indexes in the list count "My Location"; saved places start at 1.
            guard let from = source.first, from > 0 else { return }
            let to = max(destination, 1)
            model.moveSaved(from: from - 1, to: (to > from ? to - 1 : to) - 1)
        }
    }

    @ViewBuilder private var results: some View {
        if model.isSearching {
            ProgressView().frame(maxWidth: .infinity)
        }
        if let message = model.searchMessage {
            Text(message).foregroundStyle(.secondary)
        }
        ForEach(model.searchResults, id: \.self) { place in
            Button {
                // On failure the sheet stays, with the reason above.
                if model.add(place) {
                    query = ""
                    isPresented = false
                }
            } label: {
                VStack(alignment: .leading) {
                    Text(place.name).font(.body)
                    if let subtitle = place.subtitle {
                        Text(subtitle).font(.subheadline).foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint("Adds \(place.name) to your places")
            .accessibilityIdentifier("places.result.\(place.name)")
        }
    }
}

/// One place in the list: name, local time or "My Location", condition and
/// the current temperature with today's high and low, over its sky.
private struct PlaceCard: View {
    let page: WeatherPage
    @Environment(\.weatherUnits) private var units

    var body: some View {
        let overview = page.overview
        let report = page.report
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 2) {
                Text(page.isMyLocation ? "My Location" : (page.place?.name ?? "—"))
                    .font(.title2.weight(.bold))
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
                Text(subtitle)
                    .font(.subheadline.weight(.medium))
                Spacer(minLength: 12)
                if let report {
                    Text(WeatherFormat.describe(report.current.condition, isDay: overview?.isDay ?? true).text)
                        .font(.subheadline.weight(.medium))
                }
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 2) {
                if let report {
                    Text(WeatherFormat.temperature(report.current.temperatureC, units))
                        .font(.system(size: 50, weight: .light))
                }
                Spacer(minLength: 0)
                if let today = overview?.today {
                    Text("H:\(WeatherFormat.temperature(today.highC, units))  L:\(WeatherFormat.temperature(today.lowC, units))")
                        .font(.subheadline.weight(.medium))
                }
            }
        }
        .foregroundStyle(.white)
        .shadow(color: .black.opacity(0.2), radius: 3, y: 1)
        .padding(16)
        .frame(minHeight: 116)
        .background {
            SkyBackground(scene: overview?.scene ?? .cloudy, isDay: overview?.isDay ?? true, animated: false)
        }
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .accessibilityElement(children: .combine)
    }

    private var subtitle: String {
        if page.isMyLocation { return page.place?.name ?? "Not found yet" }
        if let report = page.report {
            return WeatherFormat.localClock(utcOffsetSeconds: report.utcOffsetSeconds)
        }
        return page.place?.subtitle ?? ""
    }
}

/// Saved places that couldn't be read: say so, and (only for a damaged
/// file) offer to start a new list. The old file is always kept.
private struct SavedPlacesProblemRow: View {
    let problem: SavedDataProblem
    let startNew: () -> Void
    @State private var confirming = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            switch problem {
            case .unreadable:
                Text("Your saved places can't be read, so they aren't shown. They've been kept as they are.")
                Button("Start a New List") { confirming = true }
                    .accessibilityIdentifier("places.startNew")
            case .fromNewerVersion:
                Text("Your saved places were saved by a newer version of Weather. Update the app to see them.")
            case .couldNotOpen:
                Text("Your saved places couldn't be opened just now. Try again later.")
            }
        }
        .font(.subheadline)
        .accessibilityIdentifier("places.problem")
        .confirmationDialog(
            "Start a new list of places?", isPresented: $confirming, titleVisibility: .visible
        ) {
            Button("Start a New List") { startNew() }
                .accessibilityIdentifier("places.startNew.confirm")
            Button("Cancel", role: .cancel) {}
                .accessibilityIdentifier("places.startNew.cancel")
        } message: {
            Text("The old list is kept on this device, set aside, in case it can be recovered.")
        }
    }
}
