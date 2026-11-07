import SwiftUI
import WeatherBridge

/// The places list: a card per page over its own sky, search to add a city,
/// swipe to delete and Edit to reorder. Tapping a card shows its page.
struct LocationsView: View {
    @ObservedObject var model: WeatherViewModel
    @Binding var isPresented: Bool
    @State private var query = ""

    var body: some View {
        NavigationStack {
            List {
                if query.trimmingCharacters(in: .whitespaces).isEmpty {
                    places
                } else {
                    results
                }
            }
            .listStyle(.plain)
            .navigationTitle("Weather")
            .searchable(text: $query, prompt: "Search for a city")
            .onSubmit(of: .search) { model.search(query) }
            .onChange(of: query) { _, newValue in
                if newValue.trimmingCharacters(in: .whitespaces).count >= 3 { model.search(newValue) }
            }
            .toolbar {
                #if os(iOS)
                    ToolbarItem(placement: .topBarLeading) { EditButton() }
                #endif
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { isPresented = false }
                }
            }
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
                model.add(place)
                query = ""
                isPresented = false
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
        }
    }
}

/// One place in the list: name, local time or "My Location", condition and
/// the current temperature with today's high and low, over its sky.
private struct PlaceCard: View {
    let page: WeatherPage

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
                    Text(WeatherFormat.temperature(report.current.temperatureC))
                        .font(.system(size: 50, weight: .light))
                }
                Spacer(minLength: 0)
                if let today = overview?.today {
                    Text("H:\(WeatherFormat.temperature(today.highC))  L:\(WeatherFormat.temperature(today.lowC))")
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
