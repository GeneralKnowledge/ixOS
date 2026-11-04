import SwiftUI
import WeatherBridge

/// The whole app: current conditions, the next 24 hours, the week, and
/// toolbar actions for location, search and refresh/cancel.
public struct WeatherView: View {
    @StateObject private var model: WeatherViewModel
    @State private var showingSearch = false

    public init() {
        _model = StateObject(wrappedValue: WeatherViewModel())
    }

    public init(model: WeatherViewModel) {
        _model = StateObject(wrappedValue: model)
    }

    public var body: some View {
        NavigationStack {
            content
                .navigationTitle(model.place?.name ?? "Weather")
                .toolbar { toolbar }
                .refreshable { await model.refresh().value }
                .sheet(isPresented: $showingSearch) {
                    PlaceSearchView(model: model, isPresented: $showingSearch)
                }
        }
        .task { await model.start() }
    }

    @ViewBuilder private var content: some View {
        if let report = model.report {
            List {
                if let notice = model.staleNotice {
                    Label(notice, systemImage: "wifi.slash")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                if let error = model.errorMessage {
                    Label(error, systemImage: "exclamationmark.triangle")
                        .font(.footnote)
                        .foregroundStyle(.orange)
                }
                Section { CurrentSection(report: report) }
                Section("Next 24 hours") { HourlyStrip(points: report.hourly) }
                Section("7 days") {
                    ForEach(Array(report.daily.enumerated()), id: \.element.date) { index, day in
                        DailyRow(day: day, isFirst: index == 0)
                    }
                }
                Section("Details") { DetailsSection(report: report) }
            }
        } else if model.isRefreshing {
            ProgressView("Loading weather…")
        } else {
            ContentUnavailableView {
                Label("No weather yet", systemImage: "cloud.sun")
            } description: {
                Text(model.errorMessage ?? model.locationMessage ?? "Use your location or search for a city.")
            } actions: {
                Button("Use my location") { model.useCurrentLocation() }
                Button("Search for a city") { showingSearch = true }
            }
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Button {
                model.useCurrentLocation()
            } label: {
                Label("My location", systemImage: "location")
            }
            Button {
                showingSearch = true
            } label: {
                Label("Search", systemImage: "magnifyingglass")
            }
            if model.isRefreshing {
                Button {
                    model.cancelRefresh()
                } label: {
                    Label("Cancel", systemImage: "xmark.circle")
                }
            } else {
                Button {
                    model.refresh()
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(model.place == nil)
            }
        }
    }
}

private struct CurrentSection: View {
    let report: WeatherReport

    var body: some View {
        let now = report.current
        let described = WeatherFormat.describe(now.condition, isDay: now.isDay)
        VStack(alignment: .leading, spacing: 6) {
            if let subtitle = report.place.subtitle {
                Text(subtitle).font(.subheadline).foregroundStyle(.secondary)
            }
            HStack(alignment: .firstTextBaseline) {
                Image(systemName: described.symbol).symbolRenderingMode(.multicolor).font(.largeTitle)
                Text(WeatherFormat.temperature(now.temperatureC))
                    .font(.system(size: 56, weight: .thin))
            }
            Text(described.text).font(.headline)
            HStack(spacing: 12) {
                if let feels = now.apparentTemperatureC {
                    Text("Feels like \(WeatherFormat.temperature(feels))")
                }
                if let today = report.daily.first {
                    Text("H \(WeatherFormat.temperature(today.highC))  L \(WeatherFormat.temperature(today.lowC))")
                }
            }
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }
}

private struct HourlyStrip: View {
    let points: [HourlyPoint]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 16) {
                ForEach(points, id: \.time) { point in
                    VStack(spacing: 6) {
                        Text(WeatherFormat.hour(point.time)).font(.caption)
                        Image(systemName: WeatherFormat.describe(point.condition).symbol)
                            .symbolRenderingMode(.multicolor)
                        Text(WeatherFormat.temperature(point.temperatureC)).font(.callout)
                        Text(point.precipitationProbabilityPct.map { "\($0)%" } ?? " ")
                            .font(.caption2)
                            .foregroundStyle(.blue)
                    }
                }
            }
            .padding(.vertical, 4)
        }
    }
}

private struct DailyRow: View {
    let day: DailyForecast
    let isFirst: Bool

    var body: some View {
        HStack {
            Text(WeatherFormat.weekday(day.date, isFirst: isFirst)).frame(width: 56, alignment: .leading)
            Image(systemName: WeatherFormat.describe(day.condition).symbol)
                .symbolRenderingMode(.multicolor)
                .frame(width: 32)
            if let chance = day.precipitationProbabilityMaxPct, chance > 0 {
                Text("\(chance)%").font(.caption).foregroundStyle(.blue)
            }
            Spacer()
            Text(WeatherFormat.temperature(day.lowC)).foregroundStyle(.secondary)
            Text(WeatherFormat.temperature(day.highC)).frame(width: 40, alignment: .trailing)
        }
    }
}

private struct DetailsSection: View {
    let report: WeatherReport

    var body: some View {
        let now = report.current
        let today = report.daily.first
        if let wind = now.wind {
            LabeledContent("Wind") {
                Text(windText(wind))
            }
        }
        if let humidity = now.relativeHumidityPct {
            LabeledContent("Humidity", value: "\(humidity)%")
        }
        if let precipitation = today?.precipitationSumMm {
            LabeledContent("Precipitation today", value: String(format: "%.1f mm", precipitation))
        }
        if let sunrise = today?.sunrise, let sunset = today?.sunset {
            LabeledContent("Sunrise / sunset", value: "\(WeatherFormat.hour(sunrise)) / \(WeatherFormat.hour(sunset))")
        }
        if let uv = today?.uvIndexMax {
            LabeledContent("UV index (max)", value: String(format: "%.0f", uv))
        }
        if let elevation = report.elevationM {
            LabeledContent("Elevation", value: "\(Int(elevation)) m")
        }
        LabeledContent("Time zone", value: report.timezoneAbbreviation ?? report.timezone)
    }

    private func windText(_ wind: Wind) -> String {
        var text = "\(Int(wind.speedKmh.rounded())) km/h"
        if let direction = wind.directionDeg { text += " \(WeatherFormat.compass(direction))" }
        if let gusts = wind.gustsKmh { text += ", gusts \(Int(gusts.rounded()))" }
        return text
    }
}

private struct PlaceSearchView: View {
    @ObservedObject var model: WeatherViewModel
    @Binding var isPresented: Bool
    @State private var query = ""

    var body: some View {
        NavigationStack {
            List {
                if model.isSearching {
                    ProgressView()
                }
                if let message = model.searchMessage {
                    Text(message).foregroundStyle(.secondary)
                }
                ForEach(model.searchResults, id: \.self) { place in
                    Button {
                        model.select(place)
                        isPresented = false
                    } label: {
                        VStack(alignment: .leading) {
                            Text(place.name)
                            if let subtitle = place.subtitle {
                                Text(subtitle).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Search")
            .searchable(text: $query, prompt: "City name")
            .onSubmit(of: .search) { model.search(query) }
            .onChange(of: query) { _, newValue in
                if newValue.count >= 3 { model.search(newValue) }
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { isPresented = false }
                }
            }
        }
    }
}
