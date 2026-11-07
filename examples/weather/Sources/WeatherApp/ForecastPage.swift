import SwiftUI
import WeatherBridge

/// One place: the big current-conditions header over its sky, then frosted
/// cards for the summary and hours, the days ahead, and details.
struct ForecastPage: View {
    let page: WeatherPage
    @ObservedObject var model: WeatherViewModel
    let showLocations: () -> Void

    var body: some View {
        let overview = page.overview
        ZStack {
            SkyBackground(scene: overview?.scene ?? .cloudy, isDay: overview?.isDay ?? true)
                .ignoresSafeArea()
                .animation(.easeInOut(duration: 0.8), value: overview?.scene)
            if let report = page.report, let overview {
                let palette = SkyPalette(scene: overview.scene, isDay: overview.isDay)
                ScrollView {
                    VStack(spacing: 12) {
                        Header(page: page, report: report, overview: overview)
                        if let message = page.errorMessage ?? page.staleNotice {
                            Notice(text: message, palette: palette)
                        }
                        if !overview.timeline.isEmpty || !overview.summary.isEmpty {
                            HourlyCard(overview: overview, palette: palette)
                        }
                        if !overview.days.isEmpty {
                            DailyCard(overview: overview, current: report.current.temperatureC, palette: palette)
                        }
                        DetailsGrid(report: report, overview: overview, windUnit: model.windUnit, palette: palette)
                        Footer()
                    }
                    .padding(.horizontal, 16)
                    .padding(.bottom, 24)
                    .frame(maxWidth: 640)
                    .frame(maxWidth: .infinity)
                }
                .scrollIndicators(.hidden)
                .refreshable { await model.refresh(page.id).value }
            } else if page.isRefreshing {
                VStack(spacing: 16) {
                    if page.isMyLocation {
                        Text("MY LOCATION").font(.footnote.weight(.semibold))
                    }
                    Text(page.place?.name ?? "Finding you…").font(.largeTitle)
                    ProgressView().controlSize(.large).tint(.white)
                }
                .foregroundStyle(.white)
            } else {
                EmptyPage(page: page, model: model, showLocations: showLocations)
            }
        }
    }
}

// MARK: Header

private struct Header: View {
    let page: WeatherPage
    let report: WeatherReport
    let overview: WeatherOverview

    var body: some View {
        let described = WeatherFormat.describe(report.current.condition, isDay: overview.isDay)
        VStack(spacing: 2) {
            if page.isMyLocation {
                Text("MY LOCATION")
                    .font(.footnote.weight(.semibold))
                    .tracking(0.6)
            }
            Text(report.place.name)
                .font(.system(.largeTitle, weight: .regular))
                .lineLimit(1)
                .minimumScaleFactor(0.6)
            BigTemperature(celsius: report.current.temperatureC)
            Text(described.text)
                .font(.title3.weight(.medium))
            if let today = overview.today {
                Text("H:\(WeatherFormat.temperature(today.highC))  L:\(WeatherFormat.temperature(today.lowC))")
                    .font(.title3.weight(.medium))
            }
        }
        .multilineTextAlignment(.center)
        .foregroundStyle(.white)
        .shadow(color: .black.opacity(0.18), radius: 6, y: 1)
        .padding(.top, 36)
        .padding(.bottom, 28)
        .accessibilityElement(children: .combine)
    }
}

/// "9°" with the degree sign hanging off the number, so the digits sit in
/// the middle of the screen.
private struct BigTemperature: View {
    let celsius: Double
    @ScaledMetric(relativeTo: .largeTitle) private var scaled: CGFloat = 96

    var body: some View {
        let size = min(scaled, 150)
        let number = "\(Int(celsius.rounded()))"
        Text(number == "-0" ? "0" : number)
            .font(.system(size: size, weight: .thin))
            .overlay(alignment: .topTrailing) {
                Text("°")
                    .font(.system(size: size, weight: .thin))
                    .fixedSize()
                    .alignmentGuide(.trailing) { d in d[.leading] - size * 0.02 }
            }
            .accessibilityLabel(WeatherFormat.temperature(celsius))
    }
}

// MARK: Cards

/// The frosted rounded card every section sits on, tinted by the sky.
private struct Card<Content: View>: View {
    let title: String?
    let systemImage: String?
    let palette: SkyPalette
    let content: Content

    init(
        _ title: String? = nil, systemImage: String? = nil, palette: SkyPalette,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.systemImage = systemImage
        self.palette = palette
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let title {
                Label(title.uppercased(), systemImage: systemImage ?? "circle")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.6))
                    .padding(.horizontal, 16)
                    .padding(.top, 12)
                    .padding(.bottom, 8)
                    .accessibilityAddTraits(.isHeader)
                Divider().overlay(.white.opacity(0.25)).padding(.leading, 16)
            }
            content
        }
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(.ultraThinMaterial)
                .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).fill(palette.cardTint))
        }
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
    }
}

private struct Notice: View {
    let text: String
    let palette: SkyPalette

    var body: some View {
        Card(palette: palette) {
            Label(text, systemImage: "wifi.slash")
                .font(.footnote)
                .padding(12)
        }
    }
}

/// The summary sentence over the hourly strip, as one card.
private struct HourlyCard: View {
    let overview: WeatherOverview
    let palette: SkyPalette

    var body: some View {
        Card(palette: palette) {
            if !overview.summary.isEmpty {
                Text(overview.summary)
                    .font(.subheadline)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .fixedSize(horizontal: false, vertical: true)
                if !overview.timeline.isEmpty {
                    Divider().overlay(.white.opacity(0.25)).padding(.leading, 16)
                }
            }
            if !overview.timeline.isEmpty {
                ScrollView(.horizontal) {
                    HStack(alignment: .top, spacing: 4) {
                        ForEach(Array(overview.timeline.enumerated()), id: \.offset) { _, entry in
                            TimelineCell(entry: entry)
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 12)
                }
                .scrollIndicators(.hidden)
            }
        }
    }
}

private struct TimelineCell: View {
    let entry: TimelineEntry

    var body: some View {
        VStack(spacing: 8) {
            Text(label).font(.subheadline.weight(.semibold))
            VStack(spacing: 1) {
                Image(systemName: symbol)
                    .symbolRenderingMode(.multicolor)
                    .font(.title3)
                    .frame(height: 26)
                if let chance {
                    Text("\(chance)%")
                        .font(.caption2.weight(.bold))
                        .foregroundStyle(Color(red: 0.5, green: 0.85, blue: 1))
                }
            }
            .frame(height: 42, alignment: .top)
            Text(value).font(.title3.weight(.medium))
        }
        .frame(minWidth: 52)
        .padding(.horizontal, 2)
        .accessibilityElement(children: .combine)
    }

    private var label: String {
        switch entry {
        case let .hour(point, isNow): return isNow ? "Now" : String(WeatherFormat.hour(point.time).prefix(2))
        case let .sunrise(time), let .sunset(time): return WeatherFormat.hour(time)
        }
    }

    private var symbol: String {
        switch entry {
        case let .hour(point, _): return WeatherFormat.describe(point.condition, isDay: point.isDay).symbol
        case .sunrise: return "sunrise.fill"
        case .sunset: return "sunset.fill"
        }
    }

    private var chance: UInt8? {
        guard case let .hour(point, _) = entry, WeatherFormat.isWet(point.condition),
            let pct = point.precipitationProbabilityPct, pct >= 10
        else { return nil }
        return pct
    }

    private var value: String {
        switch entry {
        case let .hour(point, _): return WeatherFormat.temperature(point.temperatureC)
        case .sunrise: return "Sunrise"
        case .sunset: return "Sunset"
        }
    }
}

private struct DailyCard: View {
    let overview: WeatherOverview
    let current: Double
    let palette: SkyPalette

    var body: some View {
        Card("\(overview.days.count)-Day Forecast", systemImage: "calendar", palette: palette) {
            VStack(spacing: 0) {
                ForEach(Array(overview.days.enumerated()), id: \.element.date) { index, day in
                    if index > 0 {
                        Divider().overlay(.white.opacity(0.25))
                    }
                    DayRow(
                        day: day,
                        isToday: day.date == overview.localDate,
                        current: day.date == overview.localDate ? current : nil,
                        rangeLow: overview.rangeLowC,
                        rangeHigh: overview.rangeHighC)
                }
            }
            .padding(.horizontal, 16)
        }
    }
}

private struct DayRow: View {
    let day: DailyForecast
    let isToday: Bool
    let current: Double?
    let rangeLow: Double
    let rangeHigh: Double

    var body: some View {
        HStack(spacing: 10) {
            Text(WeatherFormat.weekday(day.date, isFirst: isToday))
                .font(.title3.weight(.medium))
                .frame(width: 64, alignment: .leading)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            VStack(spacing: 0) {
                Image(systemName: WeatherFormat.describe(day.condition).symbol)
                    .symbolRenderingMode(.multicolor)
                    .font(.title3)
                if WeatherFormat.isWet(day.condition), let pct = day.precipitationProbabilityMaxPct, pct >= 10 {
                    Text("\(pct)%")
                        .font(.caption2.weight(.bold))
                        .foregroundStyle(Color(red: 0.5, green: 0.85, blue: 1))
                }
            }
            .frame(width: 40)
            Text(WeatherFormat.temperature(day.lowC))
                .font(.title3.weight(.medium))
                .foregroundStyle(.white.opacity(0.6))
                .frame(width: 44, alignment: .trailing)
            TemperatureBar(
                low: day.lowC, high: day.highC, rangeLow: rangeLow, rangeHigh: rangeHigh, current: current)
            Text(WeatherFormat.temperature(day.highC))
                .font(.title3.weight(.medium))
                .frame(width: 44, alignment: .trailing)
        }
        .padding(.vertical, 12)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "\(WeatherFormat.weekday(day.date, isFirst: isToday)): \(WeatherFormat.describe(day.condition).text), "
                + "low \(WeatherFormat.temperature(day.lowC)), high \(WeatherFormat.temperature(day.highC))")
    }
}

/// A day's low-to-high span on the scale shared by all days, coloured from
/// cold to warm, with a dot for the current temperature on today's row.
struct TemperatureBar: View {
    let low: Double
    let high: Double
    let rangeLow: Double
    let rangeHigh: Double
    var current: Double?

    var body: some View {
        GeometryReader { geo in
            let width = geo.size.width
            let span = max(rangeHigh - rangeLow, 1)
            let x0 = width * (low - rangeLow) / span
            let x1 = width * (high - rangeLow) / span
            ZStack(alignment: .leading) {
                Capsule().fill(.black.opacity(0.18))
                Capsule()
                    .fill(
                        LinearGradient(
                            colors: Self.colors(from: low, to: high), startPoint: .leading, endPoint: .trailing)
                    )
                    .frame(width: max(x1 - x0, 5))
                    .offset(x: min(x0, width - 5))
                if let current {
                    let x = width * (min(max(current, rangeLow), rangeHigh) - rangeLow) / span
                    Circle()
                        .fill(.white)
                        .overlay(Circle().stroke(.black.opacity(0.25), lineWidth: 1.5))
                        .frame(width: 7, height: 7)
                        .offset(x: min(max(x - 3.5, 0), width - 7))
                }
            }
            .frame(height: 5)
            .frame(maxHeight: .infinity)
        }
        .frame(height: 7)
    }

    /// Cold blue through green and yellow to hot red.
    static func color(for celsius: Double) -> Color {
        let stops: [(Double, (Double, Double, Double))] = [
            (-10, (0.35, 0.45, 1.0)), (0, (0.35, 0.70, 1.0)), (10, (0.40, 0.85, 0.80)),
            (18, (0.85, 0.88, 0.35)), (25, (1.0, 0.70, 0.25)), (32, (1.0, 0.40, 0.25)),
        ]
        guard let upper = stops.firstIndex(where: { $0.0 >= celsius }) else {
            let c = stops.last!.1
            return Color(red: c.0, green: c.1, blue: c.2)
        }
        guard upper > 0 else {
            let c = stops[0].1
            return Color(red: c.0, green: c.1, blue: c.2)
        }
        let (t0, a) = stops[upper - 1]
        let (t1, b) = stops[upper]
        let f = (celsius - t0) / (t1 - t0)
        return Color(red: a.0 + (b.0 - a.0) * f, green: a.1 + (b.1 - a.1) * f, blue: a.2 + (b.2 - a.2) * f)
    }

    static func colors(from low: Double, to high: Double) -> [Color] {
        [color(for: low), color(for: (low + high) / 2), color(for: high)]
    }
}

// MARK: Details

private struct DetailsGrid: View {
    let report: WeatherReport
    let overview: WeatherOverview
    let windUnit: WindUnit
    let palette: SkyPalette

    var body: some View {
        let now = report.current
        let today = overview.today
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 12) {
            if let feels = now.apparentTemperatureC {
                Tile("Feels like", systemImage: "thermometer.medium", palette: palette,
                     value: WeatherFormat.temperature(feels),
                     note: feelsNote(feels: feels, actual: now.temperatureC, windy: (now.wind?.speedKmh ?? 0) > 15))
            }
            if let wind = now.wind {
                Tile("Wind", systemImage: "wind", palette: palette,
                     value: WeatherFormat.speed(kmh: wind.speedKmh, unit: windUnit),
                     note: windNote(wind))
            }
            if let humidity = now.relativeHumidityPct {
                Tile("Humidity", systemImage: "humidity", palette: palette,
                     value: "\(humidity)%",
                     note: humidity >= 85 ? "The air is close to saturated." : "Relative humidity now.")
            }
            if let total = today?.precipitationSumMm {
                Tile("Precipitation", systemImage: "drop.fill", palette: palette,
                     value: String(format: "%.1f mm", total),
                     note: "Expected today.")
            }
            if let uv = today?.uvIndexMax {
                Tile("UV index", systemImage: "sun.max.fill", palette: palette,
                     value: "\(Int(uv.rounded()))",
                     note: "\(WeatherFormat.uvLevel(uv)) at its peak today.")
            }
            if let today, let sunrise = today.sunrise, let sunset = today.sunset {
                Tile("Sunset", systemImage: "sunset.fill", palette: palette,
                     value: WeatherFormat.hour(sunset),
                     note: "Sunrise: \(WeatherFormat.hour(sunrise)).")
            }
        }
    }

    private func feelsNote(feels: Double, actual: Double, windy: Bool) -> String {
        if feels < actual - 1.5 { return windy ? "Wind makes it feel colder." : "Feels colder than it is." }
        if feels > actual + 1.5 { return "Humidity makes it feel warmer." }
        return "Similar to the actual temperature."
    }

    private func windNote(_ wind: Wind) -> String {
        var parts: [String] = []
        if let gusts = wind.gustsKmh { parts.append("Gusts \(WeatherFormat.speed(kmh: gusts, unit: windUnit))") }
        if let direction = wind.directionDeg { parts.append("from the \(WeatherFormat.compass(direction))") }
        return parts.isEmpty ? "Wind speed now." : parts.joined(separator: ", ") + "."
    }
}

private struct Tile: View {
    let title: String
    let systemImage: String
    let palette: SkyPalette
    let value: String
    let note: String

    init(_ title: String, systemImage: String, palette: SkyPalette, value: String, note: String) {
        self.title = title
        self.systemImage = systemImage
        self.palette = palette
        self.value = value
        self.note = note
    }

    var body: some View {
        Card(palette: palette) {
            VStack(alignment: .leading, spacing: 6) {
                Label(title.uppercased(), systemImage: systemImage)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.6))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                Text(value)
                    .font(.system(.title, weight: .regular))
                    .lineLimit(1)
                    .minimumScaleFactor(0.6)
                Spacer(minLength: 0)
                Text(note)
                    .font(.footnote)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(14)
            .frame(maxWidth: .infinity, minHeight: 140, alignment: .topLeading)
        }
        .accessibilityElement(children: .combine)
    }
}

private struct Footer: View {
    var body: some View {
        // Open-Meteo's data is CC BY 4.0, which asks for this credit.
        Link("Weather data by Open-Meteo.com", destination: URL(string: "https://open-meteo.com/")!)
            .font(.footnote)
            .foregroundStyle(.white.opacity(0.7))
            .padding(.top, 8)
    }
}

// MARK: Empty and error states

private struct EmptyPage: View {
    let page: WeatherPage
    @ObservedObject var model: WeatherViewModel
    let showLocations: () -> Void

    var body: some View {
        ContentUnavailableView {
            Label(
                page.isMyLocation ? "Weather where you are" : (page.place?.name ?? "No weather yet"),
                systemImage: page.isMyLocation ? "location.circle" : "cloud.sun")
        } description: {
            Text(message)
        } actions: {
            if page.isMyLocation {
                Button("Use My Location") { model.useCurrentLocation() }
                    .buttonStyle(.borderedProminent)
            } else {
                Button("Try Again") { model.refresh(page.id) }
                    .buttonStyle(.borderedProminent)
            }
            Button("Search for a City", action: showLocations)
        }
        .foregroundStyle(.white)
    }

    private var message: String {
        if page.isMyLocation, let location = model.locationMessage { return location }
        return page.errorMessage ?? "Use your location or search for a city."
    }
}
