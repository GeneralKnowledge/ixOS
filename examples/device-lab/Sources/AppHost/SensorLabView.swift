import ForgeUI
import SwiftUI

/// Spatial Sensor Lab: the phone as a live 3D object, the magnetic field
/// as a vector with its strength, and how far the field has moved from a
/// baseline you set. Shows field anomaly only: a magnetometer can't tell how
/// far away a magnet is, or what an object is.
struct SensorLabView: View {
    let model: SensorLabModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        List {
            Section {
                SensorLabCanvas(model: model)
                    .frame(maxWidth: .infinity)
                    .frame(height: 320)
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
            } footer: {
                Text(canvasNote).forgeText(.note)
            }
            statusSection
            fieldSection
            anomalySection
            orientationSection
        }
        .groupedList()
        .navigationTitle("Spatial Sensor Lab")
        .inlineNavigationTitle()
        .onAppear { model.start() }
        .onDisappear { model.stop() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { model.stop() } else if phase == .active { model.start() }
        }
    }

    private var canvasNote: String {
        if model.availability.magneticNorth {
            "Seen from the south, looking north. Turn the phone and the model follows; the blue arrow is the magnetic field."
        } else {
            "This device has no magnetic north reference, so the model's heading drifts and there is no field to show."
        }
    }

    // MARK: Sections

    @ViewBuilder private var statusSection: some View {
        switch model.phase {
        case .unavailable(let reason), .failed(let reason):
            Section { Text(reason).forgeText(.note) }
        case .stopped:
            Section { Text("Paused.").forgeText(.note) }
        case .running:
            if model.sample == nil {
                Section { Text("Starting…").forgeText(.note) }
            }
        }
    }

    @ViewBuilder private var fieldSection: some View {
        if let sample = model.sample, model.availability.magneticNorth {
            Section {
                HStack(alignment: .firstTextBaseline) {
                    if model.fieldReady, let strength = model.strength {
                        Text(Format.number(strength, digits: 1)).forgeText(.figure)
                        Text("µT").forgeText(.figureCaption)
                    } else {
                        Text("Move the phone in a figure 8 to calibrate the compass.")
                            .forgeText(.note)
                    }
                    Spacer()
                    ForgeBadge(sample.calibration.name, tone: calibrationTone(sample.calibration))
                }
                .accessibilityElement(children: .combine)
                if model.fieldReady, let field = sample.field {
                    LabeledContent("Phone Axes") {
                        Text(vector(field)).font(.footnote).monospacedDigit()
                    }
                    if let inclination = model.inclinationDegrees {
                        LabeledContent("Dips Below Horizon", value: Format.degrees(inclination))
                    }
                }
            } header: {
                Text("Magnetic Field")
            } footer: {
                Text("Calibrated by Core Motion, without the phone's own magnetism. Earth's field is 25 to 65 µT.")
                    .forgeText(.note)
            }
        }
    }

    @ViewBuilder private var anomalySection: some View {
        if model.availability.magneticNorth, model.sample != nil {
            Section {
                if let collecting = model.collecting {
                    ProgressView(
                        "Setting the baseline. Hold still, away from metal.",
                        value: Double(collecting), total: Double(SensorLabModel.baselineSamples))
                } else if model.baseline != nil, let anomaly = model.anomaly {
                    HStack(alignment: .firstTextBaseline) {
                        Text(Format.number(anomaly, digits: 1)).forgeText(.figure)
                        Text("µT from baseline").forgeText(.figureCaption)
                        Spacer()
                        ForgeBadge(SensorLabModel.describe(anomaly: anomaly), tone: anomalyTone(anomaly))
                    }
                    .accessibilityElement(children: .combine)
                    AnomalyChart(values: model.history, color: theme.colors.tone(.info).solid)
                        .frame(height: 64)
                    if let change = model.strengthChange {
                        LabeledContent("Strength Change", value: "\(Format.signed(change, digits: 1)) µT")
                    }
                    if let peak = model.peakAnomaly {
                        LabeledContent("Peak", value: "\(Format.number(peak, digits: 1)) µT")
                    }
                } else {
                    Text("Set a baseline, then move the phone near an object. The change shows here.")
                        .forgeText(.note)
                }
                HStack {
                    Button(model.baseline == nil ? "Set Baseline" : "Reset Baseline") { model.setBaseline() }
                        .buttonStyle(.forgePrimary)
                        .disabled(!model.fieldReady || model.collecting != nil)
                        .accessibilityIdentifier("sensorLab.setBaseline")
                    if model.baseline != nil {
                        Button("Clear") { model.clearBaseline() }
                            .buttonStyle(.forgeSecondary)
                            .accessibilityIdentifier("sensorLab.clearBaseline")
                    }
                }
            } header: {
                Text("Field Anomaly")
            } footer: {
                Text(
                    "How far the field has moved from the baseline, the last 30 seconds in the graph. A magnet shows strongly, steel a little, aluminium and copper not at all. It can't tell how far away something is, or what it is."
                )
                .forgeText(.note)
            }
        }
    }

    @ViewBuilder private var orientationSection: some View {
        if let s = model.sample {
            Section {
                LabeledContent("Pitch", value: Format.radiansAsDegrees(s.pitch))
                LabeledContent("Roll", value: Format.radiansAsDegrees(s.roll))
                LabeledContent(
                    model.availability.magneticNorth ? "Yaw from Magnetic North" : "Yaw",
                    value: Format.radiansAsDegrees(s.yaw))
                LabeledContent("Gravity") {
                    Text(vector(s.gravity, digits: 2, unit: "g")).font(.footnote).monospacedDigit()
                }
                LabeledContent("Movement", value: "\(Format.number(s.userAcceleration.length, digits: 2)) g")
                LabeledContent("Rate", value: "\(Format.number(model.rateHz, digits: 0)) Hz")
            } header: {
                Text("Orientation")
            } footer: {
                Text("Core Motion's fused attitude: accelerometer, gyroscope and magnetometer together.")
                    .forgeText(.note)
            }
        }
    }

    // MARK: Helpers

    @Environment(\.forgeTheme) private var theme

    private func vector(_ v: SensorVector, digits: Int = 0, unit: String = "µT") -> String {
        "x \(Format.signed(v.x, digits: digits))  y \(Format.signed(v.y, digits: digits))  z \(Format.signed(v.z, digits: digits)) \(unit)"
    }

    private func calibrationTone(_ c: FieldCalibration) -> ForgeTone {
        switch c {
        case .uncalibrated: .warning
        case .low: .warning
        case .medium: .info
        case .high: .success
        }
    }

    private func anomalyTone(_ anomaly: Double) -> ForgeTone {
        switch anomaly {
        case ..<3: .neutral
        case ..<10: .info
        case ..<50: .warning
        default: .danger
        }
    }
}

/// The 3D view: the compass ring, the phone, the field and baseline arrows.
private struct SensorLabCanvas: View {
    let model: SensorLabModel
    @Environment(\.forgeTheme) private var theme

    private let scene = SensorLabScene()

    /// One arrow ready to draw: its line and head, colour, whether dashed,
    /// and how far behind the phone's centre its tip is.
    private struct Arrow {
        var line: Path
        var head: Path
        var color: Color
        var dashed: Bool
        var depth: Double
    }

    var body: some View {
        // Read here, in body, so Observation redraws the canvas on each sample.
        let rotation = model.sample?.rotation ?? .identity
        let field = model.fieldReady ? model.sample?.worldField : nil
        let baseline = model.baseline
        Canvas { context, size in
            let unit = min(size.width, size.height) / 4.6
            let centre = CGPoint(x: size.width / 2, y: size.height / 2)
            func point(_ p: SensorLabScene.Projected) -> CGPoint {
                CGPoint(x: centre.x + p.x * unit, y: centre.y + p.y * unit)
            }
            func polygon(_ corners: [SensorLabScene.Projected]) -> Path {
                var path = Path()
                path.addLines(corners.map(point))
                path.closeSubpath()
                return path
            }

            // The compass ring under the phone.
            let ringZ = -1.3
            var ring = Path()
            ring.addLines(scene.ring(radius: 1.7, z: ringZ).map(point))
            context.stroke(ring, with: .color(theme.colors.separator), lineWidth: 1)
            for (label, direction) in [("N", SensorVector(1, 0, 0)), ("E", SensorVector(0, -1, 0)),
                                       ("S", SensorVector(-1, 0, 0)), ("W", SensorVector(0, 1, 0))] {
                let at = point(scene.project(direction * 1.95 + SensorVector(0, 0, ringZ)))
                context.draw(
                    Text(label).font(.caption2.weight(.semibold))
                        .foregroundStyle(label == "N" ? theme.colors.tone(.danger).text : theme.colors.textSecondary),
                    at: at)
            }

            // Arrows behind the phone first, the phone, then those in front.
            var wanted: [(SensorVector, Double, Color, Bool)] = []
            if let field {
                wanted.append((field, SensorLabScene.arrowLength(microtesla: field.length),
                               theme.colors.tone(.info).solid, false))
            }
            if let baseline {
                wanted.append((baseline.field, SensorLabScene.arrowLength(microtesla: baseline.strength),
                               theme.colors.textTertiary, true))
            }
            var arrows: [Arrow] = []
            for (direction, length, color, dashed) in wanted {
                guard let ends = scene.arrow(direction, length: length),
                    let paths = arrowPaths(from: point(ends.tail), to: point(ends.tip))
                else { continue }
                arrows.append(Arrow(line: paths.0, head: paths.1, color: color, dashed: dashed, depth: ends.tip.depth))
            }
            for arrow in arrows where arrow.depth > 0 {
                context.stroke(arrow.line, with: .color(arrow.color),
                               style: StrokeStyle(lineWidth: 3, lineCap: .round, dash: arrow.dashed ? [5, 5] : []))
                context.stroke(arrow.head, with: .color(arrow.color), style: StrokeStyle(lineWidth: 3, lineCap: .round))
            }

            for face in scene.phoneFaces(rotation) {
                let path = polygon(face.corners)
                switch face.kind {
                case .screen:
                    context.fill(path, with: .color(Color(white: 0.08 + 0.12 * face.light)))
                case .back:
                    context.fill(path, with: .color(Color(white: 0.35 + 0.4 * face.light)))
                case .edge:
                    context.fill(path, with: .color(Color(white: 0.25 + 0.45 * face.light)))
                }
                context.stroke(path, with: .color(.black.opacity(0.35)), lineWidth: 0.5)
            }
            let h = SensorLabScene.halfSize
            // The display, lit with the accent, and the Dynamic Island at
            // its top, so you can tell top from bottom.
            if let display = scene.detail(
                scene.rectangle(z: h.z * 1.01, h, inset: 0.05), normal: SensorVector(0, 0, 1), rotation: rotation)
            {
                context.fill(polygon(display), with: .color(theme.colors.accent.solid.opacity(0.55)))
                let island = [
                    SensorVector(-0.13, 0.84, h.z * 1.02), SensorVector(0.13, 0.84, h.z * 1.02),
                    SensorVector(0.13, 0.9, h.z * 1.02), SensorVector(-0.13, 0.9, h.z * 1.02),
                ]
                if let island = scene.detail(island, normal: SensorVector(0, 0, 1), rotation: rotation) {
                    context.fill(polygon(island), with: .color(.black))
                }
            }
            // The camera bump on the back, top left as you look at the back.
            let bump = [
                SensorVector(0.06, 0.52, -h.z * 1.02), SensorVector(0.42, 0.52, -h.z * 1.02),
                SensorVector(0.42, 0.92, -h.z * 1.02), SensorVector(0.06, 0.92, -h.z * 1.02),
            ]
            if let bump = scene.detail(bump, normal: SensorVector(0, 0, -1), rotation: rotation) {
                context.fill(polygon(bump), with: .color(Color(white: 0.15)))
            }

            for arrow in arrows where arrow.depth <= 0 {
                context.stroke(arrow.line, with: .color(arrow.color),
                               style: StrokeStyle(lineWidth: 3, lineCap: .round, dash: arrow.dashed ? [5, 5] : []))
                context.stroke(arrow.head, with: .color(arrow.color), style: StrokeStyle(lineWidth: 3, lineCap: .round))
            }
        }
        .accessibilityElement()
        .accessibilityLabel("Phone orientation and magnetic field")
        .accessibilityValue(accessibilityValue)
        .accessibilityIdentifier("sensorLab.scene")
    }

    private var accessibilityValue: String {
        guard let s = model.sample else { return "No reading yet" }
        var parts = [
            "Pitch \(Format.radiansAsDegrees(s.pitch))",
            "Roll \(Format.radiansAsDegrees(s.roll))",
            "Yaw \(Format.radiansAsDegrees(s.yaw))",
        ]
        if model.fieldReady, let strength = model.strength {
            parts.append("Field \(Format.number(strength, digits: 0)) microtesla")
        }
        return parts.joined(separator: ", ")
    }

    /// A line and an arrowhead, or nil when the arrow is too short to see.
    private func arrowPaths(from tail: CGPoint, to tip: CGPoint) -> (Path, Path)? {
        let dx = tip.x - tail.x
        let dy = tip.y - tail.y
        let length = (dx * dx + dy * dy).squareRoot()
        guard length > 4 else { return nil }
        var line = Path()
        line.move(to: tail)
        line.addLine(to: tip)
        let ux = dx / length
        let uy = dy / length
        let size = min(12, length / 3)
        var head = Path()
        head.move(to: CGPoint(x: tip.x - size * (ux - uy * 0.6), y: tip.y - size * (uy + ux * 0.6)))
        head.addLine(to: tip)
        head.addLine(to: CGPoint(x: tip.x - size * (ux + uy * 0.6), y: tip.y - size * (uy - ux * 0.6)))
        return (line, head)
    }
}

/// The anomaly over the last 30 seconds, from zero up.
private struct AnomalyChart: View {
    let values: [Double]
    let color: Color

    var body: some View {
        Canvas { context, size in
            guard values.count > 1 else { return }
            let top = max(values.max() ?? 0, 10)
            let step = size.width / CGFloat(SensorLabModel.historyLength - 1)
            let start = size.width - CGFloat(values.count - 1) * step
            var path = Path()
            for (index, value) in values.enumerated() {
                let p = CGPoint(
                    x: start + CGFloat(index) * step,
                    y: size.height * (1 - CGFloat(value / top)))
                if index == 0 { path.move(to: p) } else { path.addLine(to: p) }
            }
            context.stroke(path, with: .color(color), style: StrokeStyle(lineWidth: 1.5, lineJoin: .round))
        }
        .accessibilityHidden(true)
    }
}
