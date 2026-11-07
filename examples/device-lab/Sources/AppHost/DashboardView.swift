@preconcurrency import ForgeFFI
import SwiftUI

/// Everything the phone tells an app about itself, live, in one list.
struct DashboardView: View {
    let model: LabModel

    private var s: LabSnapshot { model.snapshot }

    var body: some View {
        List {
            networkSection
            bluetoothSection
            powerSection
            storageSection
            motionSection
            compassSection
            barometerSection
            locationSection
            deviceSection
            Section {
                NavigationLink {
                    LimitsView()
                } label: {
                    Label("What iOS Keeps from Apps", systemImage: "lock.shield")
                }
            } footer: {
                Text("Wi-Fi name, device name, carrier, signal strength and more.")
            }
        }
        .groupedList()
        .navigationTitle("Device Lab")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Reset", systemImage: "arrow.counterclockwise") { model.reset() }
                    .help("Reset peaks, ranges and distance")
            }
        }
    }

    // MARK: Network

    @ViewBuilder private var networkSection: some View {
        let network = s.network
        Section {
            if !network.available {
                Note("Connectivity is not available in this build.")
            } else if let status = network.status {
                LabeledContent {
                    Text(status.summary)
                } label: {
                    Label("Status", systemImage: status.symbol)
                }
                if status.interfaces.count > 1 {
                    LabeledContent("Routes", value: status.interfaces.map(\.name).joined(separator: ", "))
                }
                if let reason = status.offlineReason {
                    LabeledContent("Why", value: reason.name)
                }
                LabeledContent("Metered", value: status.isExpensive ? "Yes" : "No")
                LabeledContent("Low Data Mode", value: status.isConstrained ? "On" : "Off")
                LabeledContent("IPv4 · IPv6 · DNS") {
                    Text([status.supportsIpv4, status.supportsIpv6, status.supportsDns]
                        .map { $0 ? "Yes" : "No" }.joined(separator: " · "))
                }
                LabeledContent("Large Downloads", value: network.unmetered ? "Would start now" : "Would wait for Wi-Fi")
                LabeledContent("Wi-Fi Name") {
                    Text("Needs a paid team").foregroundStyle(.secondary)
                }
                NavigationLink {
                    NetworkLogView(changes: network.changes)
                } label: {
                    LabeledContent("Recent Changes", value: "\(network.changes.count)")
                }
            } else {
                Note("Waiting for the first report…")
            }
        } header: {
            Text("Network")
        }
    }

    // MARK: Bluetooth

    @ViewBuilder private var bluetoothSection: some View {
        Section {
            if !s.bluetooth.available {
                Note("Bluetooth is not available in this build.")
            } else if !s.permissions.bluetooth.allowsUse {
                if let state = s.bluetooth.state, state == .unauthorized || state == .unsupported {
                    LabeledContent("Bluetooth", value: state.name)
                }
                PermissionPrompt(
                    status: s.permissions.bluetooth,
                    allowTitle: "Allow Bluetooth Access",
                    explanation: "iOS only tells an app whether Bluetooth is on once the app may use it."
                ) { await model.request(.bluetooth) }
            } else {
                LabeledContent {
                    Text(s.bluetooth.state?.name ?? "Checking…")
                } label: {
                    Label("Bluetooth", systemImage: "dot.radiowaves.left.and.right")
                }
            }
        } header: {
            Text("Bluetooth")
        }
    }

    // MARK: Power

    @ViewBuilder private var powerSection: some View {
        Section {
            if let status = s.device.status {
                if let level = status.batteryLevel {
                    LabeledContent {
                        Text("\(Format.percent(Double(level))) · \(status.batteryState.name)")
                            .monospacedDigit()
                    } label: {
                        Label("Battery", systemImage: batterySymbol(level, status.batteryState))
                    }
                } else {
                    LabeledContent("Battery", value: status.batteryState.name)
                }
                LabeledContent("Low Power Mode", value: status.lowPowerMode ? "On" : "Off")
                LabeledContent {
                    Text(status.thermalState.name)
                        .foregroundStyle(thermalColor(status.thermalState))
                } label: {
                    Label("Temperature", systemImage: "thermometer.medium")
                }
                LabeledContent("Heavy Work") {
                    Text(s.device.heavyWorkDeferred?.name ?? "Would start now")
                        .multilineTextAlignment(.trailing)
                }
                if let brightness = status.screenBrightness {
                    LabeledContent("Screen Brightness", value: Format.percent(Double(brightness)))
                }
            } else {
                Note(s.device.available ? "Reading…" : "Device status is not available in this build.")
            }
        } header: {
            Text("Battery & Power")
        } footer: {
            Text("Heavy Work is Forge's default rule for big downloads and on-device AI.")
        }
    }

    // MARK: Storage and memory

    @ViewBuilder private var storageSection: some View {
        if let status = s.device.status {
            Section("Storage & Memory") {
                if let total = status.storageTotalBytes, let free = status.storageAvailableBytes, total > 0 {
                    VStack(alignment: .leading, spacing: 6) {
                        LabeledContent("Storage", value: "\(Format.storage(free)) free of \(Format.storage(total))")
                        ProgressView(value: Double(total - min(free, total)), total: Double(total))
                    }
                }
                if let footprint = status.memoryFootprintBytes {
                    LabeledContent("App Memory Used", value: Format.memory(footprint))
                }
                if let available = status.memoryAvailableBytes {
                    LabeledContent("App Memory Left", value: Format.memory(available))
                }
                if let info = s.device.info {
                    LabeledContent("Device Memory", value: Format.memory(info.physicalMemoryBytes))
                }
            }
        }
    }

    // MARK: Motion

    @ViewBuilder private var motionSection: some View {
        Section {
            NavigationLink {
                MotionView(model: model)
            } label: {
                VStack(spacing: 10) {
                    ReadingRow(
                        title: "Acceleration", symbol: "move.3d",
                        value: s.accelerometer.magnitude.map { "\(Format.number($0)) g" } ?? "–",
                        history: s.accelerometer.history, color: .orange)
                    ReadingRow(
                        title: "Rotation", symbol: "gyroscope",
                        value: s.gyroscope.magnitude.map { "\(Format.number($0)) rad/s" } ?? "–",
                        history: s.gyroscope.history, color: .purple)
                    ReadingRow(
                        title: "Magnetic Field", symbol: "location.north.circle",
                        value: s.magnetometer.magnitude.map { "\(Format.number($0, digits: 0)) µT" } ?? "–",
                        history: s.magnetometer.history, color: .teal)
                    LabeledContent {
                        Text(s.attitude.tiltDegrees.map { Format.degrees($0) } ?? "–").monospacedDigit()
                    } label: {
                        Label("Tilt", systemImage: "level")
                    }
                }
                .padding(.vertical, 4)
            }
        } header: {
            Text("Motion")
        } footer: {
            if !s.accelerometer.available && !s.gyroscope.available {
                Text("No motion sensors here.")
            } else {
                Text("\(Format.number(s.accelerometer.rateHz, digits: 0)) updates a second.")
            }
        }
    }

    // MARK: Compass

    @ViewBuilder private var compassSection: some View {
        Section("Compass") {
            NavigationLink {
                CompassView(model: model)
            } label: {
                LabeledContent {
                    if let heading = CompassView.heading(s) {
                        Text("\(Format.cardinal(heading.degrees)) \(Format.degrees(heading.degrees))")
                            .monospacedDigit()
                    } else {
                        Text("Calibrating…").foregroundStyle(.secondary)
                    }
                } label: {
                    Label("Heading", systemImage: "safari")
                }
            }
        }
    }

    // MARK: Barometer

    @ViewBuilder private var barometerSection: some View {
        let b = s.barometer
        Section {
            if !b.available {
                Note("This device has no barometer.")
            } else if !s.permissions.motion.allowsUse {
                PermissionPrompt(
                    status: s.permissions.motion,
                    allowTitle: "Allow Motion & Fitness",
                    explanation: "The barometer is part of Motion & Fitness, which asks first."
                ) { await model.request(.motion) }
            } else if let error = b.error {
                Note(error)
            } else {
                ReadingRow(
                    title: "Air Pressure", symbol: "barometer",
                    value: b.pressureKpa.map(Format.pressure) ?? "Starting…",
                    detail: range(b.minPressureKpa, b.maxPressureKpa),
                    history: b.history, color: .indigo)
                LabeledContent {
                    Text(b.relativeAltitudeM.map { Format.signed($0, digits: 1) + " m" } ?? "–")
                        .monospacedDigit()
                } label: {
                    Label("Height Change", systemImage: "arrow.up.and.down")
                }
            }
        } header: {
            Text("Barometer")
        } footer: {
            if b.available && s.permissions.motion.allowsUse {
                Text("Height change counts from when the barometer started.")
            }
        }
    }

    private func range(_ low: Double?, _ high: Double?) -> String? {
        guard let low, let high else { return nil }
        return "Range \(Format.pressure(low)) – \(Format.pressure(high))"
    }

    // MARK: Location

    @ViewBuilder private var locationSection: some View {
        let l = s.location
        Section {
            if !l.available {
                Note("Location is not available in this build.")
            } else if !s.permissions.location.allowsUse {
                PermissionPrompt(
                    status: s.permissions.location,
                    allowTitle: "Allow Location Access",
                    explanation: "Shows your position, altitude and speed while the app is open. Nothing leaves the phone."
                ) { await model.request(.location) }
            } else if let fix = l.fix {
                LabeledContent("Latitude", value: Format.coordinate(fix.latitude, positive: "N", negative: "S"))
                LabeledContent("Longitude", value: Format.coordinate(fix.longitude, positive: "E", negative: "W"))
                LabeledContent("Accuracy", value: "± \(Format.metres(fix.horizontalAccuracyM))")
                if let altitude = fix.altitudeM {
                    LabeledContent(
                        "Altitude",
                        value: "\(Format.metres(altitude)) ± \(Format.metres(fix.verticalAccuracyM ?? 0))")
                }
                if let speed = fix.speedMps {
                    LabeledContent("Speed", value: Format.speed(speed))
                }
                if let course = fix.courseDegrees {
                    LabeledContent("Course", value: "\(Format.cardinal(course)) \(Format.degrees(course))")
                }
                if let floor = fix.floor {
                    LabeledContent("Floor", value: "\(floor)")
                }
                LabeledContent("Travelled", value: Format.distance(l.distanceM))
                if let top = l.maxSpeedMps {
                    LabeledContent("Top Speed", value: Format.speed(top))
                }
                LabeledContent("Precise Location", value: l.precise ? "On" : "Off")
                if let url = URL(string: "https://maps.apple.com/?ll=\(fix.latitude),\(fix.longitude)&q=Here") {
                    Link(destination: url) {
                        Label("Show in Maps", systemImage: "map")
                    }
                }
            } else if let error = l.error {
                Note(error)
            } else {
                Note("Finding your position…")
            }
        } header: {
            Text("Location")
        } footer: {
            if l.fix != nil {
                Text("\(l.fixes) fixes. Updated \(Format.time(unixMs: l.fix?.timestampMs ?? 0)).")
            }
        }
    }

    // MARK: Device

    @ViewBuilder private var deviceSection: some View {
        if let info = s.device.info {
            Section {
                LabeledContent("Model", value: info.modelIdentifier)
                LabeledContent("System", value: "\(info.systemName) \(info.systemVersion)")
                LabeledContent("Name") {
                    Text(info.deviceName)
                }
                LabeledContent(
                    "Processor Cores",
                    value: info.activeProcessorCount == info.processorCount
                        ? "\(info.processorCount)"
                        : "\(info.activeProcessorCount) of \(info.processorCount) active")
                if info.screenWidthPoints > 0 {
                    LabeledContent(
                        "Screen",
                        value: "\(Int(info.screenWidthPoints)) × \(Int(info.screenHeightPoints)) pt @\(Int(info.screenScale))x")
                }
                if let status = s.device.status {
                    LabeledContent("Up For", value: Format.uptime(status.uptimeSeconds))
                }
                if info.isSimulator {
                    LabeledContent("Simulator", value: "Yes")
                }
            } header: {
                Text("Device")
            } footer: {
                Text("Since iOS 16 apps see the model's name, not the name you gave your phone.")
            }
        }
    }

    // MARK: Helpers

    private func batterySymbol(_ level: Float, _ state: BatteryState) -> String {
        if state == .charging { return "battery.100percent.bolt" }
        switch level {
        case ..<0.13: return "battery.0percent"
        case ..<0.38: return "battery.25percent"
        case ..<0.63: return "battery.50percent"
        case ..<0.88: return "battery.75percent"
        default: return "battery.100percent"
        }
    }

    private func thermalColor(_ state: ThermalState) -> Color {
        switch state {
        case .nominal: .primary
        case .fair: .yellow
        case .serious: .orange
        case .critical: .red
        }
    }
}

/// Every connectivity change since the app opened, newest first.
struct NetworkLogView: View {
    let changes: [ConnectivityChange]

    var body: some View {
        List(Array(changes.enumerated()), id: \.offset) { _, change in
            VStack(alignment: .leading, spacing: 2) {
                Label(change.status.summary, systemImage: change.status.symbol)
                Text(details(change))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .overlay {
            if changes.isEmpty {
                ContentUnavailableView("No Changes Yet", systemImage: "network",
                    description: Text("Turn Wi-Fi or Airplane Mode on and off to see them here."))
            }
        }
        .navigationTitle("Recent Changes")
        .inlineNavigationTitle()
    }

    private func details(_ change: ConnectivityChange) -> String {
        var parts = [Format.time(unixMs: change.atMs)]
        if change.status.isExpensive { parts.append("metered") }
        if change.status.isConstrained { parts.append("Low Data Mode") }
        if let reason = change.status.offlineReason { parts.append(reason.name) }
        return parts.joined(separator: " · ")
    }
}
