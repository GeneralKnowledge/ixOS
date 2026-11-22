import ForgeUI
import SwiftUI
import WeatherFFI

/// One place: the big current-conditions header over its sky, then frosted
/// cards for the summary and hours, the days ahead, and details.
struct ForecastPage: View {
    let page: WeatherPage
    @Bindable var model: WeatherViewModel
    let showLocations: () -> Void
    /// How far the page has scrolled, in points.
    @State private var scrolled: CGFloat = 0
    /// The height of the short "2° | Mostly Sunny" line, which is where
    /// cards pin their titles.
    @State private var condensedHeight: CGFloat = 0

    var body: some View {
        let overview = page.overview
        ZStack {
            SkyBackground(scene: overview?.scene ?? .cloudy, isDay: overview?.isDay ?? true)
                .ignoresSafeArea()
                .animation(.easeInOut(duration: 0.8), value: overview?.scene)
            if let report = page.report, let overview {
                let palette = SkyPalette(scene: overview.scene, isDay: overview.isDay)
                // The place's name stays at the top; under it the big
                // temperature fades as the page scrolls and a short line takes
                // its place, and each card keeps its title pinned under that
                // line while its contents slide away beneath it.
                VStack(spacing: 0) {
                    PlaceTitle(page: page, report: report)
                    ScrollView(.vertical) {
                        PageCards(
                            page: page, report: report, overview: overview, palette: palette, model: model,
                            headerOpacity: headerOpacity)
                            .onGeometryChange(for: CGFloat.self) { proxy in
                                -proxy.frame(in: NamedCoordinateSpace.named(forecastScrollSpace)).minY
                            } action: { offset in
                                scrolled = offset
                            }
                    }
                    .coordinateSpace(NamedCoordinateSpace.named(forecastScrollSpace))
                    .scrollIndicators(.hidden)
                    .refreshable { await model.refresh(page.id).value }
                    .environment(\.cardPinTop, condensedHeight)
                    .overlay(alignment: .top) {
                        CondensedHeader(report: report, overview: overview)
                            .opacity(condensedOpacity)
                            .onGeometryChange(for: CGFloat.self) { proxy in
                                proxy.size.height
                            } action: { height in
                                condensedHeight = height
                            }
                    }
                }
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

/// The page's scroll view, which cards measure against to pin their titles.
private let forecastScrollSpace = "forecast.scroll"

extension ForecastPage {
    /// The big header fades over the first 120 points of scrolling.
    fileprivate var headerOpacity: Double {
        1 - min(max(Double(scrolled) / 120, 0), 1)
    }

    /// The short line fades in as the big header finishes fading.
    fileprivate var condensedOpacity: Double {
        min(max((Double(scrolled) - 90) / 40, 0), 1)
    }

}

/// Everything that scrolls: the big header, then the cards.
private struct PageCards: View {
    let page: WeatherPage
    let report: WeatherReport
    let overview: WeatherOverview
    let palette: SkyPalette
    @Bindable var model: WeatherViewModel
    let headerOpacity: Double

    var body: some View {
        VStack(spacing: 12) {
            Header(report: report, overview: overview)
                .opacity(headerOpacity)
            if let message = model.notice(for: page) {
                Notice(text: message, systemImage: noticeSymbol, palette: palette)
            }
            if !overview.timeline.isEmpty || !overview.summary.isEmpty {
                HourlyCard(overview: overview, palette: palette)
            }
            if !overview.days.isEmpty {
                DailyCard(overview: overview, current: report.current.temperatureC, palette: palette)
            }
            AirQualityCard(overview: overview.airQuality, palette: palette)
            PollenCard(overview: overview.pollen, palette: palette)
            DetailsSection(details: overview.details, palette: palette)
            Footer(credits: overview.credits)
        }
        .padding(.horizontal, 16)
        .padding(.bottom, 24)
        .frame(maxWidth: 640)
        .frame(maxWidth: .infinity)
    }

    /// Wi-Fi for a failed refresh or an offline fallback, a crossed-out
    /// arrow when "My Location" couldn't be found, a clock for an old report.
    private var noticeSymbol: String {
        if page.errorMessage != nil || report.isOfflineFallback { return "wifi.slash" }
        if page.isMyLocation, model.locationMessage != nil { return "location.slash" }
        return "clock"
    }
}

// MARK: Header

/// "MY LOCATION" and the place's name, which stay at the top of the page.
private struct PlaceTitle: View {
    let page: WeatherPage
    let report: WeatherReport

    var body: some View {
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
        }
        .multilineTextAlignment(.center)
        .foregroundStyle(.white)
        .shadow(color: .black.opacity(0.18), radius: 6, y: 1)
        .padding(.horizontal, 16)
        .padding(.top, 24)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// "2° | Mostly Sunny": what the big header shrinks to once it has
/// scrolled away. VoiceOver already has it from the big header.
private struct CondensedHeader: View {
    let report: WeatherReport
    let overview: WeatherOverview
    @Environment(\.weatherUnits) private var units

    var body: some View {
        let described = WeatherFormat.describe(report.current.condition, isDay: overview.isDay)
        Text("\(WeatherFormat.temperature(report.current.temperatureC, units))  |  \(described.text)")
            .font(.title3.weight(.medium))
            .foregroundStyle(.white)
            .shadow(color: .black.opacity(0.18), radius: 6, y: 1)
            .lineLimit(1)
            .padding(.top, 2)
            .padding(.bottom, 10)
            .frame(maxWidth: .infinity)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

/// The big temperature, the condition and today's high and low; fades as
/// the page scrolls.
private struct Header: View {
    let report: WeatherReport
    let overview: WeatherOverview
    @Environment(\.weatherUnits) private var units

    var body: some View {
        let described = WeatherFormat.describe(report.current.condition, isDay: overview.isDay)
        VStack(spacing: 2) {
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

/// Where cards pin their titles, in the scroll view's coordinates: the
/// bottom of the short header line over the page.
private struct CardPinTopKey: EnvironmentKey {
    static let defaultValue: CGFloat = 0
}

extension EnvironmentValues {
    fileprivate var cardPinTop: CGFloat {
        get { self[CardPinTopKey.self] }
        set { self[CardPinTopKey.self] = newValue }
    }
}

/// The frosted rounded card every section sits on, tinted by the sky.
///
/// When its top scrolls up to the pin line, the title stays there and the
/// card shrinks from the top, its contents sliding away under the title,
/// until only the title is left; then the whole card scrolls off.
private struct Card<Content: View>: View {
    let title: String?
    let systemImage: String?
    /// A tile's smaller title, without the rule under it.
    let compact: Bool
    let palette: SkyPalette
    let content: Content
    @Environment(\.cardPinTop) private var pinTop
    @State private var top: CGFloat = .greatestFiniteMagnitude
    @State private var height: CGFloat = 0
    @State private var titleHeight: CGFloat = 0

    init(
        _ title: String? = nil, systemImage: String? = nil, compact: Bool = false, palette: SkyPalette,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.systemImage = systemImage
        self.compact = compact
        self.palette = palette
        self.content = content()
    }

    var body: some View {
        // How much of the card has gone under the pin line.
        let pushed = min(max(0, pinTop - top), max(0, height - titleHeight))
        VStack(alignment: .leading, spacing: 0) {
            if let title {
                titleView(title)
                    .onGeometryChange(for: CGFloat.self) { proxy in
                        proxy.size.height
                    } action: { height in
                        titleHeight = height
                    }
                    .offset(y: pushed)
                    .zIndex(1)
            }
            content
                .mask(alignment: .top) { Rectangle().padding(.top, pushed) }
        }
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(.ultraThinMaterial)
                .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).fill(palette.cardTint))
                .padding(.top, pushed)
        }
        .mask { RoundedRectangle(cornerRadius: 20, style: .continuous).padding(.top, pushed) }
        .onGeometryChange(for: CGRect.self) { proxy in
            proxy.frame(in: NamedCoordinateSpace.named(forecastScrollSpace))
        } action: { frame in
            top = frame.minY
            height = frame.height
        }
    }

    @ViewBuilder private func titleView(_ title: String) -> some View {
        if compact {
            Label(title.uppercased(), systemImage: systemImage ?? "circle")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white.opacity(0.6))
                .lineLimit(1)
                .minimumScaleFactor(0.8)
                .padding(.horizontal, 14)
                .padding(.top, 14)
                .padding(.bottom, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityAddTraits(.isHeader)
        } else {
            VStack(alignment: .leading, spacing: 0) {
                Label(title.uppercased(), systemImage: systemImage ?? "circle")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.6))
                    .padding(.horizontal, 16)
                    .padding(.top, 12)
                    .padding(.bottom, 8)
                    .accessibilityAddTraits(.isHeader)
                Divider().overlay(.white.opacity(0.25)).padding(.leading, 16)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct Notice: View {
    let text: String
    let systemImage: String
    let palette: SkyPalette

    var body: some View {
        // ForgeUI's notice, drawn in the sky's colours on Weather's own card.
        Card(palette: palette) {
            ForgeNotice(text, systemImage: systemImage)
                .forgeNoticeBackground(.hidden)
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

/// The detail cards. Rust works out every value, sentence and position
/// (`WeatherOverview.details`); these only lay them out: pairs of tiles,
/// with wind and the moon across the full width. A card the source sent no
/// data for is left out.
private struct DetailsSection: View {
    let details: WeatherDetails
    let palette: SkyPalette
    @Environment(\.weatherUnits) private var units

    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        VStack(spacing: 12) {
            LazyVGrid(columns: columns, spacing: 12) {
                if let feels = details.feelsLike {
                    DetailTile(
                        "Feels like", systemImage: "thermometer.medium", palette: palette,
                        value: WeatherFormat.temperature(feels.celsius, units), note: feels.note,
                        identifier: "forecast.feelsLike")
                }
                if let uv = details.uv {
                    DetailTile(
                        "UV index", systemImage: "sun.max.fill", palette: palette,
                        value: "\(Int(uv.index.rounded()))", caption: uv.label, note: uv.note,
                        identifier: "forecast.uv"
                    ) {
                        ScaleBar(position: uv.scalePosition, colors: ScaleBar.uv)
                    }
                }
            }
            if let wind = details.wind {
                WindCard(wind: wind, palette: palette)
            }
            LazyVGrid(columns: columns, spacing: 12) {
                if let sun = details.sun {
                    DetailTile(
                        sun.nextIsSunset ? "Sunset" : "Sunrise",
                        systemImage: sun.nextIsSunset ? "sunset.fill" : "sunrise.fill", palette: palette,
                        value: (sun.nextIsSunset ? sun.sunset : sun.sunrise) ?? "None today", note: sun.note,
                        identifier: "forecast.sun"
                    ) {
                        SunArc(progress: sun.progress)
                    }
                }
                if let rain = details.precipitation {
                    DetailTile(
                        "Precipitation", systemImage: "drop.fill", palette: palette,
                        value: WeatherFormat.quantity(.precipitation(mm: rain.todayMm), units), caption: "Today",
                        note: rain.note, identifier: "forecast.precipitation")
                }
                if let visibility = details.visibility {
                    DetailTile(
                        "Visibility", systemImage: "eye.fill", palette: palette,
                        value: WeatherFormat.quantity(.distance(metres: visibility.metres), units),
                        note: visibility.note, identifier: "forecast.visibility")
                }
                if let humidity = details.humidity {
                    DetailTile(
                        "Humidity", systemImage: "humidity", palette: palette,
                        value: "\(humidity.percent)%", note: humidity.note, identifier: "forecast.humidity")
                }
            }
            MoonCard(moon: details.moon, palette: palette)
            LazyVGrid(columns: columns, spacing: 12) {
                if let pressure = details.pressure {
                    let value = WeatherFormat.quantity(.pressure(hpa: pressure.hpa), units)
                    DetailTile(
                        "Pressure", systemImage: "gauge.with.dots.needle.33percent", palette: palette,
                        value: nil, spokenValue: value, note: pressure.note, identifier: "forecast.pressure"
                    ) {
                        PressureDial(position: pressure.scalePosition, trend: pressure.trend, value: value)
                            .frame(maxWidth: 130)
                            .frame(maxWidth: .infinity)
                    }
                }
            }
        }
    }
}

/// A square-ish detail card: title, the value (big), an optional caption
/// under it, an optional drawing, and Rust's sentence at the bottom.
/// VoiceOver reads it as one element.
private struct DetailTile<Graphic: View>: View {
    let title: String
    let systemImage: String
    let palette: SkyPalette
    let value: String?
    let spokenValue: String?
    let caption: String?
    let note: String
    let identifier: String
    let graphic: Graphic

    init(
        _ title: String, systemImage: String, palette: SkyPalette, value: String?, spokenValue: String? = nil,
        caption: String? = nil, note: String, identifier: String, @ViewBuilder graphic: () -> Graphic
    ) {
        self.title = title
        self.systemImage = systemImage
        self.palette = palette
        self.value = value
        self.spokenValue = spokenValue
        self.caption = caption
        self.note = note
        self.identifier = identifier
        self.graphic = graphic()
    }

    var body: some View {
        Card(title, systemImage: systemImage, compact: true, palette: palette) {
            VStack(alignment: .leading, spacing: 6) {
                if let value {
                    Text(value)
                        .font(.system(.title, weight: .regular))
                        .lineLimit(1)
                        .minimumScaleFactor(0.6)
                }
                if let caption {
                    Text(caption).font(.title3.weight(.semibold))
                }
                graphic
                Spacer(minLength: 0)
                Text(note)
                    .font(.footnote)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding([.horizontal, .bottom], 14)
            .frame(maxWidth: .infinity, minHeight: 140, alignment: .topLeading)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel([title, value ?? spokenValue, caption, note].compactMap { $0 }.joined(separator: ", "))
        .accessibilityIdentifier(identifier)
    }
}

extension DetailTile where Graphic == EmptyView {
    init(
        _ title: String, systemImage: String, palette: SkyPalette, value: String?, caption: String? = nil,
        note: String, identifier: String
    ) {
        self.init(
            title, systemImage: systemImage, palette: palette, value: value, caption: caption, note: note,
            identifier: identifier
        ) { EmptyView() }
    }
}

/// One line of a wide card: "29" big, then "KM/H" over "Wind".
private struct MeasureRow: View {
    let number: String
    let unit: String
    let label: String

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            Text(number).font(.title2.weight(.medium)).lineLimit(1).minimumScaleFactor(0.7)
            VStack(alignment: .leading, spacing: 0) {
                if !unit.isEmpty {
                    Text(unit.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(.white.opacity(0.7))
                }
                Text(label).font(.subheadline)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 6)
    }
}

/// "29 km/h" as ("29", "km/h").
private func splitUnit(_ text: String) -> (String, String) {
    guard let space = text.firstIndex(of: " ") else { return (text, "") }
    return (String(text[..<space]), String(text[text.index(after: space)...]))
}

private struct WindCard: View {
    let wind: WindDetail
    let palette: SkyPalette
    @Environment(\.weatherUnits) private var units

    var body: some View {
        let measured = splitUnit(WeatherFormat.speed(kmh: wind.speedKmh, units: units))
        let speed = measured.0
        let unit = measured.1
        Card("Wind", systemImage: "wind", compact: true, palette: palette) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .center, spacing: 16) {
                    VStack(spacing: 0) {
                        MeasureRow(number: speed, unit: unit, label: "Wind")
                        if let gusts = wind.gustsKmh {
                            Divider().overlay(.white.opacity(0.25))
                            let g = splitUnit(WeatherFormat.speed(kmh: gusts, units: units))
                            MeasureRow(number: g.0, unit: g.1, label: "Gusts")
                        }
                        if let direction = wind.direction {
                            Divider().overlay(.white.opacity(0.25))
                            let d = splitUnit(direction)
                            MeasureRow(number: d.0, unit: d.1, label: "Direction")
                        }
                    }
                    WindCompass(directionDeg: wind.directionDeg.map(Double.init), speed: speed, unit: unit)
                        .frame(width: 130)
                }
                Text(wind.note)
                    .font(.footnote)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding([.horizontal, .bottom], 14)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "Wind, \(WeatherFormat.speed(kmh: wind.speedKmh, units: units)), \(wind.note)")
        .accessibilityIdentifier("forecast.wind")
    }
}

private struct MoonCard: View {
    let moon: MoonDetail
    let palette: SkyPalette

    var body: some View {
        let name = WeatherFormat.moonName(moon.phase.name)
        Card(name, systemImage: WeatherFormat.moonSymbol(moon.phase.name), compact: true, palette: palette) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .center, spacing: 16) {
                    VStack(spacing: 0) {
                        row("Illumination", "\(moon.phase.illuminationPct)%")
                        Divider().overlay(.white.opacity(0.25))
                        row("Moonrise", moon.moonrise ?? "None today")
                        Divider().overlay(.white.opacity(0.25))
                        row("Moonset", moon.moonset ?? "None today")
                        Divider().overlay(.white.opacity(0.25))
                        row("Next full moon", fullMoon)
                    }
                    MoonDisc(phase: moon.phase)
                        .frame(width: 110)
                }
            }
            .padding([.horizontal, .bottom], 14)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "Moon, \(name), \(moon.phase.illuminationPct)% lit. "
                + (moon.moonrise.map { "Moonrise \($0). " } ?? "")
                + (moon.moonset.map { "Moonset \($0). " } ?? "")
                + moon.note)
        .accessibilityIdentifier("forecast.moon")
    }

    private var fullMoon: String {
        switch moon.daysToFullMoon {
        case 0: return "Today"
        case 1: return "Tomorrow"
        default: return "\(moon.daysToFullMoon) days"
        }
    }

    private func row(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
            Spacer()
            Text(value).foregroundStyle(.white.opacity(0.75))
        }
        .font(.subheadline)
        .padding(.vertical, 7)
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

/// The index big with its band's name, a marker on the colour scale, and
/// Rust's comparison with yesterday. The pollutants and how far the
/// forecast runs fold out under "Details".
private struct AirQualityCard: View {
    let overview: AirQualityOverview
    let palette: SkyPalette
    @State private var expanded = false

    var body: some View {
        Card("Air Quality", systemImage: "aqi.medium", palette: palette) {
            VStack(alignment: .leading, spacing: 10) {
                if let message = sectionMessage(overview.state, what: "Air quality") {
                    Text(message).font(.subheadline)
                        .accessibilityIdentifier("forecast.airQuality.message")
                } else if let index = overview.index {
                    VStack(alignment: .leading, spacing: 10) {
                        VStack(alignment: .leading, spacing: 0) {
                            Text("\(index.value)").font(.system(.largeTitle, weight: .regular))
                            Text(index.label).font(.title3.weight(.semibold))
                        }
                        ScaleBar(
                            position: overview.scalePosition ?? (Double(index.level) - 0.5) / 6,
                            colors: ScaleBar.airQuality)
                        Text(overview.comparison ?? scaleName(index.scale))
                            .font(.subheadline)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .accessibilityElement(children: .combine)
                    if expanded {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(scaleName(index.scale))
                            if let now = overview.now, let text = pollutants(now) { Text(text) }
                            if let until = overview.coversUntil {
                                Text("Forecast until \(WeatherFormat.weekday(String(until.prefix(10)))) \(WeatherFormat.hour(until)).")
                            }
                        }
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.8))
                    }
                    Button {
                        withAnimation(.easeInOut(duration: 0.2)) { expanded.toggle() }
                    } label: {
                        Label(expanded ? "Less" : "Details", systemImage: expanded ? "chevron.up" : "chevron.down")
                            .font(.footnote.weight(.semibold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.white.opacity(0.8))
                    .accessibilityLabel(expanded ? "Hide air quality details" : "Show air quality details")
                    .accessibilityIdentifier("forecast.airQuality.details")
                } else {
                    Text("No reading for this hour.").font(.subheadline)
                }
                SourceLine(state: overview.state, source: overview.source)
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("forecast.airQuality")
    }

    private func scaleName(_ scale: AirQualityScale) -> String {
        scale == .european ? "European Air Quality Index" : "US Air Quality Index"
    }

    private func pollutants(_ h: AirQualityHour) -> String? {
        var parts: [String] = []
        if let v = h.pm25 { parts.append("PM2.5 \(Int(v.rounded()))") }
        if let v = h.pm10 { parts.append("PM10 \(Int(v.rounded()))") }
        if let v = h.ozone { parts.append("O₃ \(Int(v.rounded()))") }
        if let v = h.nitrogenDioxide { parts.append("NO₂ \(Int(v.rounded()))") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ") + " µg/m³"
    }
}

/// "Updated 2 h ago" when the data is kept, then the source, small.
private struct SourceLine: View {
    let state: SectionState
    let source: SourceCredit?

    var body: some View {
        let parts = [sectionAge(state), source?.name].compactMap { $0 }
        if !parts.isEmpty {
            Text(parts.joined(separator: " · "))
                .font(.caption2)
                .foregroundStyle(.white.opacity(0.6))
        }
    }
}

/// Rust's one-line summary, then only the species in the air today.
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
                } else {
                    // Empty when the source has hours ahead but none for now.
                    Text(overview.summary.isEmpty ? "No reading for this hour." : overview.summary)
                        .font(.subheadline)
                        .fixedSize(horizontal: false, vertical: true)
                    if !overview.species.isEmpty {
                        VStack(spacing: 0) {
                            ForEach(overview.species, id: \.species) { s in
                                HStack {
                                    Text(WeatherFormat.pollenName(s.species))
                                    Spacer()
                                    Text("\(WeatherFormat.grains(s.nowGrainsPerM3)) now · up to \(WeatherFormat.grains(s.todayMaxGrainsPerM3))")
                                        .foregroundStyle(.white.opacity(0.8))
                                }
                                .font(.subheadline)
                                .padding(.vertical, 6)
                            }
                        }
                        Text("Grains per cubic metre.").font(.caption2).foregroundStyle(.white.opacity(0.6))
                    }
                }
                SourceLine(state: overview.state, source: overview.source)
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("forecast.pollen")
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
                // Found nowhere yet (no fix) or the forecast failed: asking
                // for the location again is the way forward. After a denial
                // iOS won't ask again, so Settings is.
                let useLocation = model.locationDenied
                    ? ForgeStateAction.openSettings(identifier: "forecast.openSettings")
                    : ForgeStateAction("Use My Location", identifier: "forecast.useLocation") {
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
