import SwiftUI
import WeatherBridge

/// Colours for one kind of sky, by day or night. Drawn by `SkyBackground`
/// and used to tint the frosted cards so they sit in the sky.
struct SkyPalette: Equatable {
    /// Top to bottom.
    var gradient: [Color]
    /// Tint laid over the cards' material.
    var cardTint: Color
    /// 0 (none) to 1 (overcast): how much drifting cloud to draw.
    var cloudCover: Double
    var cloudBrightness: Double
    var sunGlow: Bool
    var stars: Bool
    var precipitation: Precipitation?

    enum Precipitation { case rain, snow }

    init(scene: SkyScene, isDay: Bool) {
        func rgb(_ r: Double, _ g: Double, _ b: Double) -> Color { Color(red: r, green: g, blue: b) }
        sunGlow = false
        stars = false
        precipitation = nil
        cloudBrightness = isDay ? 1 : 0.55
        switch (scene, isDay) {
        case (.clear, true):
            gradient = [rgb(0.13, 0.38, 0.78), rgb(0.29, 0.56, 0.90), rgb(0.55, 0.76, 0.96)]
            cloudCover = 0.1
            sunGlow = true
        case (.clear, false):
            gradient = [rgb(0.03, 0.06, 0.16), rgb(0.08, 0.13, 0.29), rgb(0.16, 0.23, 0.42)]
            cloudCover = 0
            stars = true
        case (.partlyCloudy, true):
            gradient = [rgb(0.20, 0.42, 0.74), rgb(0.36, 0.57, 0.84), rgb(0.58, 0.74, 0.92)]
            cloudCover = 0.45
            sunGlow = true
        case (.partlyCloudy, false):
            gradient = [rgb(0.06, 0.09, 0.19), rgb(0.12, 0.17, 0.31), rgb(0.21, 0.27, 0.42)]
            cloudCover = 0.4
            stars = true
        case (.cloudy, true):
            gradient = [rgb(0.36, 0.46, 0.58), rgb(0.48, 0.57, 0.68), rgb(0.62, 0.69, 0.78)]
            cloudCover = 0.9
        case (.cloudy, false):
            gradient = [rgb(0.11, 0.13, 0.19), rgb(0.18, 0.21, 0.28), rgb(0.27, 0.30, 0.37)]
            cloudCover = 0.8
        case (.fog, true):
            gradient = [rgb(0.47, 0.52, 0.58), rgb(0.60, 0.64, 0.68), rgb(0.70, 0.72, 0.75)]
            cloudCover = 1
        case (.fog, false):
            gradient = [rgb(0.16, 0.18, 0.22), rgb(0.25, 0.27, 0.31), rgb(0.33, 0.35, 0.39)]
            cloudCover = 1
        case (.rain, true):
            gradient = [rgb(0.27, 0.33, 0.42), rgb(0.37, 0.44, 0.53), rgb(0.48, 0.55, 0.63)]
            cloudCover = 1
            precipitation = .rain
        case (.rain, false):
            gradient = [rgb(0.08, 0.10, 0.15), rgb(0.14, 0.17, 0.23), rgb(0.21, 0.25, 0.31)]
            cloudCover = 0.9
            precipitation = .rain
        case (.snow, true):
            gradient = [rgb(0.36, 0.44, 0.57), rgb(0.47, 0.55, 0.66), rgb(0.60, 0.66, 0.75)]
            cloudCover = 0.9
            precipitation = .snow
        case (.snow, false):
            gradient = [rgb(0.13, 0.16, 0.24), rgb(0.21, 0.25, 0.34), rgb(0.30, 0.34, 0.43)]
            cloudCover = 0.8
            precipitation = .snow
        case (.thunderstorm, _):
            gradient = isDay
                ? [rgb(0.18, 0.19, 0.27), rgb(0.26, 0.27, 0.36), rgb(0.36, 0.37, 0.46)]
                : [rgb(0.06, 0.06, 0.11), rgb(0.12, 0.12, 0.19), rgb(0.19, 0.19, 0.27)]
            cloudCover = 1
            cloudBrightness = isDay ? 0.7 : 0.4
            precipitation = .rain
        }
        cardTint = gradient[0].opacity(0.45)
    }
}

/// The sky behind a page: a gradient for the scene and time of day, with a
/// sun glow, stars, slowly drifting clouds, and rain or snow. All drawn, no
/// image assets. Motion stops when Reduce Motion is on.
struct SkyBackground: View {
    let palette: SkyPalette
    /// False draws one still frame (the small cards in the places list).
    var animated = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(scene: SkyScene, isDay: Bool, animated: Bool = true) {
        palette = SkyPalette(scene: scene, isDay: isDay)
        self.animated = animated
    }

    var body: some View {
        ZStack {
            LinearGradient(colors: palette.gradient, startPoint: .top, endPoint: .bottom)
            if palette.sunGlow {
                GeometryReader { geo in
                    RadialGradient(
                        colors: [.white.opacity(0.85), Color(red: 1, green: 0.95, blue: 0.8).opacity(0.35), .clear],
                        center: UnitPoint(x: 0.82, y: 0.07),
                        startRadius: 4,
                        endRadius: geo.size.width * 0.75)
                }
            }
            if palette.stars { StarField() }
            TimelineView(.animation(minimumInterval: 1 / 30, paused: reduceMotion || !animated)) { timeline in
                let t = timeline.date.timeIntervalSinceReferenceDate
                ZStack {
                    if palette.cloudCover > 0 {
                        CloudLayer(cover: palette.cloudCover, brightness: palette.cloudBrightness, time: t)
                    }
                    if let precipitation = palette.precipitation {
                        PrecipitationLayer(kind: precipitation, time: t)
                    }
                }
            }
        }
        .accessibilityHidden(true)
    }
}

/// A fixed, seeded scatter of stars in the upper half.
private struct StarField: View {
    var body: some View {
        Canvas { context, size in
            var rng = SeededRandom(seed: 7)
            for _ in 0..<90 {
                let x = rng.next() * size.width
                let y = rng.next() * size.height * 0.55
                let r = 0.5 + rng.next() * 1.1
                let alpha = 0.35 + rng.next() * 0.6
                context.fill(
                    Path(ellipseIn: CGRect(x: x, y: y, width: r * 2, height: r * 2)),
                    with: .color(.white.opacity(alpha)))
            }
        }
    }
}

/// Soft blurred cloud banks drifting slowly to the right.
private struct CloudLayer: View {
    let cover: Double
    let brightness: Double
    let time: TimeInterval

    var body: some View {
        Canvas { context, size in
            context.addFilter(.blur(radius: 28))
            var rng = SeededRandom(seed: 3)
            let count = Int(4 + cover * 10)
            for i in 0..<count {
                let speed = 4 + rng.next() * 8 // points per second
                let width = size.width * (0.5 + rng.next() * 0.7)
                let height = width * (0.28 + rng.next() * 0.2)
                let span = size.width + width
                let x = (rng.next() * span + time * speed).truncatingRemainder(dividingBy: span) - width
                // Most cloud sits behind the header, thinning towards the bottom.
                let y = pow(rng.next(), 1.6) * size.height * 0.75 - height * 0.3
                let alpha = (0.18 + rng.next() * 0.3) * (0.4 + cover * 0.6)
                let shade = brightness * (0.85 + Double(i % 3) * 0.05)
                context.fill(
                    Path(ellipseIn: CGRect(x: x, y: y, width: width, height: height)),
                    with: .color(Color(white: shade).opacity(alpha)))
            }
        }
    }
}

/// Light rain streaks or snowflakes.
private struct PrecipitationLayer: View {
    let kind: SkyPalette.Precipitation
    let time: TimeInterval

    var body: some View {
        Canvas { context, size in
            var rng = SeededRandom(seed: 11)
            let count = kind == .rain ? 70 : 60
            for _ in 0..<count {
                let speed = kind == .rain ? 380 + rng.next() * 220 : 22 + rng.next() * 26
                let x0 = rng.next() * size.width
                let y = (rng.next() * size.height + time * speed).truncatingRemainder(dividingBy: size.height)
                switch kind {
                case .rain:
                    let length = 10 + rng.next() * 12
                    var path = Path()
                    path.move(to: CGPoint(x: x0, y: y))
                    path.addLine(to: CGPoint(x: x0 - length * 0.18, y: y + length))
                    context.stroke(path, with: .color(.white.opacity(0.18 + rng.next() * 0.15)), lineWidth: 1)
                case .snow:
                    let sway = sin(time * 0.8 + x0) * 12
                    let r = 1.2 + rng.next() * 2
                    context.fill(
                        Path(ellipseIn: CGRect(x: x0 + sway, y: y, width: r * 2, height: r * 2)),
                        with: .color(.white.opacity(0.5 + rng.next() * 0.4)))
                }
            }
        }
    }
}

/// Deterministic pseudo-random numbers in 0..<1, so the sky looks the same
/// on every frame and every launch.
private struct SeededRandom {
    private var state: UInt64

    init(seed: UInt64) { state = seed &* 0x9E37_79B9_7F4A_7C15 | 1 }

    mutating func next() -> Double {
        state ^= state << 13
        state ^= state >> 7
        state ^= state << 17
        return Double(state % 1_000_000) / 1_000_000
    }
}
