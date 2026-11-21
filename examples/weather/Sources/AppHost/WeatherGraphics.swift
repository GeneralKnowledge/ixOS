import SwiftUI
import WeatherFFI

// Small drawings for the detail cards. Each draws a value Rust worked out
// (a position from 0 to 1, a direction, a phase); none decides anything.
// All are decoration next to text that says the same, so VoiceOver skips
// them.

/// A colour scale with a dot where the value sits (air quality, UV).
struct ScaleBar: View {
    /// 0 (left) to 1 (right).
    let position: Double
    let colors: [Color]

    static let airQuality: [Color] = [
        .green, .yellow, .orange, .red, .purple, Color(red: 0.5, green: 0, blue: 0.15),
    ]
    static let uv: [Color] = [.green, .yellow, .orange, .red, .purple]

    var body: some View {
        GeometryReader { geo in
            let width = geo.size.width
            let dot: CGFloat = 9
            ZStack(alignment: .leading) {
                Capsule()
                    .fill(LinearGradient(colors: colors, startPoint: .leading, endPoint: .trailing))
                    .frame(height: 5)
                Circle()
                    .fill(.white)
                    .overlay(Circle().stroke(.black.opacity(0.3), lineWidth: 1.5))
                    .frame(width: dot, height: dot)
                    .offset(x: min(max(0, width * position - dot / 2), width - dot))
            }
            .frame(width: width, height: dot)
        }
        .frame(height: 9)
        .accessibilityHidden(true)
    }
}

/// A compass dial: ticks, the four points, and an arrow from where the
/// wind comes from to where it blows, with the speed in the middle.
struct WindCompass: View {
    /// Where the wind comes from, degrees clockwise from north.
    let directionDeg: Double?
    let speed: String
    let unit: String

    var body: some View {
        ZStack {
            Canvas { context, size in
                let r = min(size.width, size.height) / 2
                let c = CGPoint(x: size.width / 2, y: size.height / 2)
                func point(_ degrees: Double, _ radius: CGFloat) -> CGPoint {
                    let a = degrees * .pi / 180
                    return CGPoint(x: c.x + sin(a) * radius, y: c.y - cos(a) * radius)
                }
                for i in 0..<72 {
                    let degrees = Double(i) * 5
                    // Leave room for the letters.
                    if i % 18 == 0 { continue }
                    let long = i % 6 == 0
                    var tick = Path()
                    tick.move(to: point(degrees, r - 2))
                    tick.addLine(to: point(degrees, r - (long ? 9 : 6)))
                    context.stroke(tick, with: .color(.white.opacity(long ? 0.7 : 0.35)), lineWidth: 1)
                }
                for (degrees, letter) in [(0.0, "N"), (90.0, "E"), (180.0, "S"), (270.0, "W")] {
                    context.draw(
                        Text(letter).font(.caption2.weight(.bold)).foregroundStyle(.white),
                        at: point(degrees, r - 6))
                }
                guard let from = directionDeg else { return }
                let tail = point(from, r - 16)
                let head = point(from + 180, r - 16)
                var line = Path()
                line.move(to: tail)
                line.addLine(to: head)
                context.stroke(line, with: .color(.white), style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                context.fill(Path(ellipseIn: CGRect(x: tail.x - 4.5, y: tail.y - 4.5, width: 9, height: 9)), with: .color(.white))
                // The arrowhead, pointing where the wind goes.
                let dx = head.x - tail.x, dy = head.y - tail.y
                let length = max(sqrt(dx * dx + dy * dy), 1)
                let (ux, uy) = (dx / length, dy / length)
                var arrow = Path()
                arrow.move(to: head)
                arrow.addLine(to: CGPoint(x: head.x - ux * 9 + uy * 5, y: head.y - uy * 9 - ux * 5))
                arrow.addLine(to: CGPoint(x: head.x - ux * 9 - uy * 5, y: head.y - uy * 9 + ux * 5))
                arrow.closeSubpath()
                context.fill(arrow, with: .color(.white))
            }
            VStack(spacing: 0) {
                Text(speed).font(.title3.weight(.semibold))
                Text(unit).font(.caption2)
            }
            .padding(10)
            .background(Circle().fill(.ultraThinMaterial))
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }
}

/// The sun's path through the day: a curve over the horizon, with the sun
/// on it while it is up.
struct SunArc: View {
    /// 0 at sunrise, 1 at sunset; nil while the sun is down.
    let progress: Double?

    var body: some View {
        Canvas { context, size in
            let horizon = size.height * 0.62
            let amplitude = size.height * 0.34
            // The curve crosses the horizon at a quarter and three quarters
            // of the width: sunrise and sunset.
            func y(_ t: Double) -> CGFloat { horizon - amplitude * (-cos(2 * .pi * t)) }
            var curve = Path()
            for step in 0...60 {
                let t = Double(step) / 60
                let p = CGPoint(x: size.width * t, y: y(t))
                if step == 0 { curve.move(to: p) } else { curve.addLine(to: p) }
            }
            context.stroke(curve, with: .color(.white.opacity(0.55)), lineWidth: 2)
            var line = Path()
            line.move(to: CGPoint(x: 0, y: horizon))
            line.addLine(to: CGPoint(x: size.width, y: horizon))
            context.stroke(line, with: .color(.white.opacity(0.45)), lineWidth: 1)
            if let progress {
                let t = 0.25 + 0.5 * min(max(progress, 0), 1)
                let sun = CGPoint(x: size.width * t, y: y(t))
                context.fill(
                    Path(ellipseIn: CGRect(x: sun.x - 12, y: sun.y - 12, width: 24, height: 24)),
                    with: .color(.white.opacity(0.25)))
                context.fill(
                    Path(ellipseIn: CGRect(x: sun.x - 6, y: sun.y - 6, width: 12, height: 12)),
                    with: .color(.white))
            }
        }
        .frame(height: 56)
        .accessibilityHidden(true)
    }
}

/// A pressure dial: an open ring of ticks from low to high with the
/// reading marked, and the trend arrow and value in the middle.
struct PressureDial: View {
    /// 0 (low) to 1 (high).
    let position: Double
    let trend: PressureTrend?
    let value: String

    var body: some View {
        ZStack {
            Canvas { context, size in
                let r = min(size.width, size.height) / 2
                let c = CGPoint(x: size.width / 2, y: size.height / 2)
                // From 225° (bottom left) clockwise to 135° (bottom right).
                func point(_ fraction: Double, _ radius: CGFloat) -> CGPoint {
                    let a = (225 + 270 * fraction) * .pi / 180
                    return CGPoint(x: c.x + sin(a) * radius, y: c.y - cos(a) * radius)
                }
                for i in 0...40 {
                    let f = Double(i) / 40
                    var tick = Path()
                    tick.move(to: point(f, r - 2))
                    tick.addLine(to: point(f, r - 10))
                    context.stroke(tick, with: .color(.white.opacity(0.4)), lineWidth: 1.5)
                }
                let f = min(max(position, 0), 1)
                var marker = Path()
                marker.move(to: point(f, r))
                marker.addLine(to: point(f, r - 14))
                context.stroke(marker, with: .color(.white), style: StrokeStyle(lineWidth: 4, lineCap: .round))
                context.draw(Text("Low").font(.caption2).foregroundStyle(.white.opacity(0.8)), at: point(0, r - 2).applying(.init(translationX: 6, y: 10)))
                context.draw(Text("High").font(.caption2).foregroundStyle(.white.opacity(0.8)), at: point(1, r - 2).applying(.init(translationX: -6, y: 10)))
            }
            VStack(spacing: 2) {
                if let trend {
                    Image(systemName: Self.symbol(trend)).font(.subheadline.weight(.bold))
                }
                Text(value).font(.subheadline.weight(.semibold)).minimumScaleFactor(0.7).lineLimit(1)
            }
            .padding(.horizontal, 20)
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }

    static func symbol(_ trend: PressureTrend) -> String {
        switch trend {
        case .rising: return "arrow.up"
        case .falling: return "arrow.down"
        case .steady: return "equal"
        }
    }
}

/// The moon, drawn: the lit part by the phase, with the terminator as half
/// an ellipse. Waxing is lit on the right (as seen from the northern
/// hemisphere).
struct MoonDisc: View {
    let phase: MoonPhase

    var body: some View {
        Canvas { context, size in
            let r = min(size.width, size.height) / 2 - 1
            let c = CGPoint(x: size.width / 2, y: size.height / 2)
            let disc = Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
            context.fill(disc, with: .color(.white.opacity(0.14)))
            let k = Double(phase.illuminationPct) / 100
            let waxing = phase.cycleFraction < 0.5
            let side: CGFloat = waxing ? 1 : -1
            // Terminator half-width: positive bulges toward the lit edge
            // (a crescent), negative away from it (gibbous).
            let e = r * CGFloat(1 - 2 * k)
            var lit = Path()
            lit.move(to: CGPoint(x: c.x, y: c.y - r))
            for step in 0...40 {
                // The lit limb, top to bottom.
                let a = -Double.pi / 2 + Double.pi * Double(step) / 40
                lit.addLine(to: CGPoint(x: c.x + side * r * CGFloat(cos(a)), y: c.y + r * CGFloat(sin(a))))
            }
            for step in 0...40 {
                // Back up along the terminator.
                let a = Double.pi / 2 - Double.pi * Double(step) / 40
                lit.addLine(to: CGPoint(x: c.x + side * e * CGFloat(cos(a)), y: c.y + r * CGFloat(sin(a))))
            }
            lit.closeSubpath()
            context.fill(
                lit,
                with: .linearGradient(
                    Gradient(colors: [.white, Color(white: 0.82)]),
                    startPoint: CGPoint(x: c.x - r, y: c.y - r),
                    endPoint: CGPoint(x: c.x + r, y: c.y + r)))
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }
}
