import ForgeUI
import SwiftUI
import WeatherFFI

/// One place: the big current-conditions header over its sky, then frosted
/// cards for the summary and hours, the days ahead, and details.
struct ForecastPage: View {
    let page: WeatherPage
    @Bindable var model: WeatherViewModel
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
                        if let message = model.notice(for: page) {
                            Notice(text: message, systemImage: noticeSymbol(report), palette: palette)
                        }
                        if !overview.timeline.isEmpty || !overview.summary.isEmpty {
                            HourlyCard(overview: overview, palette: palette)
                        }
                        if !overview.days.isEmpty {
                            DailyCard(overview: overview, current: report.current.temperatureC, palette: palette)
                        }
                        AirQualityCard(overview: overview.airQuality, palette: palette)
                        PollenCard(overview: overview.pollen, palette: palette)
                        DetailsGrid(report: report, overview: overview, palette: palette)
                        Footer(credits: overview.credits)
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

extension ForecastPage {
    /// Wi-Fi for a failed refresh or an offline fallback, a crossed-out
    /// arrow when "My Location" couldn't be found, a clock for an old report.
    fileprivate func noticeSymbol(_ report: WeatherReport) -> String {
        if page.errorMessage != nil || report.isOfflineFallback { return "wifi.slash" }
        if page.isMyLocation, model.locationMessage != nil { return "location.slash" }
        return "clock"
    }
}

// MARK: Header

private struct Header: View {
    let page: WeatherPage
    let report: WeatherReport
    let overview: WeatherOverview
    @Environment(\.weatherUnits) private var units

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
                Text("H:\(WeatherFormat.temperature(today.highC, units))  L:\(WeatherFormat.temperature(today.lowC, units))")
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
    @Environment(\.weatherUnits) private var units
    @ScaledMetric(relativeTo: .largeTitle) private var scaled: CGFloat = 96

    var body: some View {
        let size = min(scaled, 150)
        // "12°" in the person's unit, drawn without its degree sign.
        let number = String(WeatherFormat.temperature(celsius, units).dropLast())
        Text(number)
            .font(.system(size: size, weight: .thin))
            .overlay(alignment: .topTrailing) {
                Text("°")
                    .font(.system(size: size, weight: .thin))
                    .fixedSize()
                    .alignmentGuide(.trailing) { d in d[.leading] - size * 0.02 }
            }
            .accessibilityLabel(WeatherFormat.temperature(celsius, units))
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
    let systemImage: String
    let palette: SkyPalette

    var body: some View {
        Card(palette: palette) {
            Label(text, systemImage: systemImage)
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
    @Environment(\.weatherUnits) private var units

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
        case let .hour(point, _): return WeatherFormat.temperature(point.temperatureC, units)
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
    @Environment(\.weatherUnits) private var units

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
            Text(WeatherFormat.temperature(day.lowC, units))
                .font(.title3.weight(.medium))
                .foregroundStyle(.white.opacity(0.6))
                .frame(width: 44, alignment: .trailing)
            TemperatureBar(
                low: day.lowC, high: day.highC, rangeLow: rangeLow, rangeHigh: rangeHigh, current: current)
            Text(WeatherFormat.temperature(day.highC, units))
                .font(.title3.weight(.medium))
                .frame(width: 44, alignment: .trailing)
        }
        .padding(.vertical, 12)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "\(WeatherFormat.weekday(day.date, isFirst: isToday)): \(WeatherFormat.describe(day.condition).text), "
                + "low \(WeatherFormat.temperature(day.lowC, units)), high \(WeatherFormat.temperature(day.highC, units))")
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
    let palette: SkyPalette
    @Environment(\.weatherUnits) private var units

    var body: some View {
        let now = report.current
        let today = overview.today
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 12) {
            if let feels = now.apparentTemperatureC {
                Tile("Feels like", systemImage: "thermometer.medium", palette: palette,
                     value: WeatherFormat.temperature(feels, units),
                     note: feelsNote(feels: feels, actual: now.temperatureC, windy: (now.wind?.speedKmh ?? 0) > 15))
            }
            if let wind = now.wind {
                Tile("Wind", systemImage: "wind", palette: palette,
                     value: WeatherFormat.speed(kmh: wind.speedKmh, units: units),
                     note: windNote(wind))
            }
            if let humidity = now.relativeHumidityPct {
                Tile("Humidity", systemImage: "humidity", palette: palette,
                     value: "\(humidity)%",
                     note: now.dewPointC.map { "The dew point is \(WeatherFormat.temperature($0, units)) right now." }
                        ?? (humidity >= 85 ? "The air is close to saturated." : "Relative humidity now."))
            }
            if let total = today?.precipitationSumMm {
                Tile("Precipitation", systemImage: "drop.fill", palette: palette,
                     value: WeatherFormat.quantity(.precipitation(mm: total), units),
                     note: "Expected today.")
            }
            if let uv = today?.uvIndexMax {
                Tile("UV index", systemImage: "sun.max.fill", palette: palette,
                     value: "\(Int((now.uvIndex ?? uv).rounded()))",
                     note: now.uvIndex == nil
                        ? "\(WeatherFormat.uvLevel(uv)) at its peak today."
                        : "\(WeatherFormat.uvLevel(now.uvIndex ?? uv)) now. Peak today \(Int(uv.rounded())).")
            }
            if let visibility = now.visibilityM {
                Tile("Visibility", systemImage: "eye.fill", palette: palette,
                     value: WeatherFormat.quantity(.distance(metres: visibility), units),
                     note: visibility < 1_000 ? "Fog or heavy precipitation." : visibility < 10_000 ? "Hazy." : "Clear view.")
            }
            if let pressure = now.pressureMslHpa {
                Tile("Pressure", systemImage: "gauge.with.dots.needle.33percent", palette: palette,
                     value: WeatherFormat.quantity(.pressure(hpa: pressure), units),
                     note: "At sea level.")
            }
            if let today, let sunrise = today.sunrise, let sunset = today.sunset {
                Tile("Sunset", systemImage: "sunset.fill", palette: palette,
                     value: WeatherFormat.hour(sunset),
                     note: "Sunrise: \(WeatherFormat.hour(sunrise))."
                        + (overview.daylightMinutes.map { " Daylight \($0 / 60) h \($0 % 60) min." } ?? ""))
            }
            Tile("Moon", systemImage: WeatherFormat.moonSymbol(overview.moon.name), palette: palette,
                 value: WeatherFormat.moonName(overview.moon.name),
                 note: "\(overview.moon.illuminationPct)% lit.")
        }
    }

    private func feelsNote(feels: Double, actual: Double, windy: Bool) -> String {
        if feels < actual - 1.5 { return windy ? "Wind makes it feel colder." : "Feels colder than it is." }
        if feels > actual + 1.5 { return "Humidity makes it feel warmer." }
        return "Similar to the actual temperature."
    }

    private func windNote(_ wind: Wind) -> String {
        var parts: [String] = []
        if let gusts = wind.gustsKmh { parts.append("Gusts \(WeatherFormat.speed(kmh: gusts, units: units))") }
        if let direction = wind.directionDeg { parts.append("from the \(WeatherFormat.compass(direction))") }
        return parts.isEmpty ? "Wind speed now." : parts.joined(separator: ", ") + "."
    }
}

// MARK: Air quality and pollen

/// The section's message when it has nothing to show, or nil when it has
/// data. "Not available here" and "couldn't load" are never the same line.
/// `notHere` says why a source can have nothing for a place.
private func sectionMessage(_ state: SectionState, what: String, notHere: String? = nil) -> String? {
    switch state {
    case .live, .cached: return nil
    case .notAvailableHere: return notHere ?? "\(what) isn't available for this place."
    case let .unavailable(problem):
        guard let problem else { return "No \(what.lowercased()) data yet." }
        return "Couldn't load \(what.lowercased()): \(WeatherFormat.problem(problem))."
    }
}

/// "Updated 2 h ago" for kept data, with why it couldn't be refreshed.
private func sectionAge(_ state: SectionState) -> String? {
    guard case let .cached(age, problem) = state else { return nil }
    let base = "Updated \(WeatherFormat.age(seconds: age)) ago"
    guard let problem else { return base + "." }
    return base + " (\(WeatherFormat.problem(problem)))."
}

private struct AirQualityCard: View {
    let overview: AirQualityOverview
    let palette: SkyPalette

    var body: some View {
        Card("Air Quality", systemImage: "aqi.medium", palette: palette) {
            VStack(alignment: .leading, spacing: 8) {
                if let message = sectionMessage(overview.state, what: "Air quality") {
                    Text(message).font(.subheadline)
                        .accessibilityIdentifier("forecast.airQuality.message")
                } else if let index = overview.index {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(index.value)").font(.system(.title, weight: .regular))
                        Text(index.label).font(.title3.weight(.medium))
                    }
                    AqiScaleBar(level: index.level)
                    Text(index.scale == .european ? "European Air Quality Index" : "US Air Quality Index")
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.7))
                    if let now = overview.now {
                        Text(pollutants(now)).font(.footnote)
                    }
                } else {
                    Text("No reading for this hour.").font(.subheadline)
                }
                if let until = overview.coversUntil, sectionMessage(overview.state, what: "") == nil {
                    Text("Forecast until \(WeatherFormat.weekday(String(until.prefix(10)))) \(WeatherFormat.hour(until)).")
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.7))
                }
                if let age = sectionAge(overview.state) {
                    Text(age).font(.footnote).foregroundStyle(.white.opacity(0.7))
                }
                if let source = overview.source {
                    Text(source.name).font(.caption2).foregroundStyle(.white.opacity(0.6))
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("forecast.airQuality")
    }

    private func pollutants(_ h: AirQualityHour) -> String {
        var parts: [String] = []
        if let v = h.pm25 { parts.append("PM2.5 \(Int(v.rounded()))") }
        if let v = h.pm10 { parts.append("PM10 \(Int(v.rounded()))") }
        if let v = h.ozone { parts.append("O₃ \(Int(v.rounded()))") }
        if let v = h.nitrogenDioxide { parts.append("NO₂ \(Int(v.rounded()))") }
        return parts.isEmpty ? "" : parts.joined(separator: " · ") + " µg/m³"
    }
}

/// Six bands, the current one marked.
private struct AqiScaleBar: View {
    let level: UInt8

    var body: some View {
        HStack(spacing: 3) {
            ForEach(1...6, id: \.self) { band in
                Capsule()
                    .fill(Self.color(band).opacity(Int(level) == band ? 1 : 0.35))
                    .frame(height: Int(level) == band ? 8 : 5)
            }
        }
        .accessibilityHidden(true)
    }

    static func color(_ band: Int) -> Color {
        switch band {
        case 1: return .green
        case 2: return .yellow
        case 3: return .orange
        case 4: return .red
        case 5: return .purple
        default: return Color(red: 0.5, green: 0, blue: 0.15)
        }
    }
}

private struct PollenCard: View {
    let overview: PollenOverview
    let palette: SkyPalette

    var body: some View {
        Card("Pollen", systemImage: "leaf.fill", palette: palette) {
            VStack(alignment: .leading, spacing: 8) {
                // Rust tells "outside the model's area" from "nothing
                // forecast now"; the source sends the same empty answer.
                if let message = sectionMessage(
                    overview.state, what: "Pollen",
                    notHere: overview.gap == .outsideModelArea
                        ? "Pollen forecasts cover Europe only, so there's none for this place."
                        : "No pollen forecast available right now. Pollen forecasts here run during the pollen season.")
                {
                    Text(message).font(.subheadline)
                        .accessibilityIdentifier("forecast.pollen.message")
                } else if overview.species.isEmpty {
                    Text("No reading for this hour.").font(.subheadline)
                } else {
                    ForEach(overview.species, id: \.species) { s in
                        HStack {
                            Text(WeatherFormat.pollenName(s.species))
                            Spacer()
                            Text("\(WeatherFormat.grains(s.nowGrainsPerM3)) now, up to \(WeatherFormat.grains(s.todayMaxGrainsPerM3)) today")
                                .foregroundStyle(.white.opacity(0.8))
                        }
                        .font(.subheadline)
                    }
                    Text("Grains per cubic metre.").font(.footnote).foregroundStyle(.white.opacity(0.7))
                }
                if let age = sectionAge(overview.state) {
                    Text(age).font(.footnote).foregroundStyle(.white.opacity(0.7))
                }
                if let source = overview.source {
                    Text(source.name).font(.caption2).foregroundStyle(.white.opacity(0.6))
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("forecast.pollen")
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
    let credits: [SourceCredit]
    #if os(iOS)
        @State private var credit: URL?
    #endif

    var body: some View {
        // Every source's credit, which their licences ask for (CC BY 4.0,
        // Copernicus). On iOS the page opens in Safari inside the app, so the
        // forecast stays put.
        VStack(spacing: 6) {
            ForEach(credits, id: \.id) { source in
                #if os(iOS)
                    Button(source.credit) { credit = URL(string: source.url) }
                        .accessibilityIdentifier("forecast.credit.\(source.id)")
                #else
                    Link(source.credit, destination: URL(string: source.url)!)
                #endif
            }
        }
        .font(.footnote)
        .multilineTextAlignment(.center)
        .foregroundStyle(.white.opacity(0.7))
        .padding(.top, 8)
        #if os(iOS)
            .forgeSafari($credit)
        #endif
    }
}

// MARK: Empty and error states

private struct EmptyPage: View {
    let page: WeatherPage
    @Bindable var model: WeatherViewModel
    let showLocations: () -> Void

    var body: some View {
        Group {
            if page.isMyLocation {
                // Found nowhere yet (no permission, no fix) or the forecast
                // failed: either way, asking for the location again is the
                // way forward.
                let useLocation = ForgeStateAction("Use My Location", identifier: "forecast.useLocation") {
                    model.useCurrentLocation()
                }
                if page.errorMessage != nil {
                    ForgeStateView.failed("Weather where you are", message: message, retry: useLocation, secondary: search)
                } else {
                    ForgeStateView.empty(
                        "Weather where you are", systemImage: "location.circle",
                        message: message, action: useLocation, secondary: search)
                }
            } else {
                let retry = ForgeStateAction("Try Again", identifier: "forecast.retry") { _ = model.refresh(page.id) }
                if page.errorMessage != nil {
                    ForgeStateView.failed(page.place?.name ?? String(localized: "No weather yet"),
                                          message: message, retry: retry, secondary: search)
                } else {
                    ForgeStateView.empty(
                        page.place?.name ?? String(localized: "No weather yet"), systemImage: "cloud.sun",
                        message: message, action: retry, secondary: search)
                }
            }
        }
        .foregroundStyle(.white)
    }

    private var search: ForgeStateAction {
        ForgeStateAction("Search for a City", identifier: "forecast.search", action: showLocations)
    }

    private var message: String {
        if page.isMyLocation, let location = model.locationMessage { return location }
        return page.errorMessage ?? "Use your location or search for a city."
    }
}
