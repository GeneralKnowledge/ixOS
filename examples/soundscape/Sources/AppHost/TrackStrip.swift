#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeUI
import SwiftUI

/// One track in the mixer: its waveform, record, mute and solo, and its
/// level, pan, tone and echo. A slider shows its value as it moves and
/// sends the whole strip to Rust when it is let go.
struct TrackStrip: View {
    let model: SoundscapeModel
    let projectID: UInt64
    let track: Track
    let isRecording: Bool
    let canRecord: Bool
    let changed: (ProjectDetail) -> Void
    let record: () -> Void
    let rename: () -> Void

    /// The strip while a slider moves; `nil` shows what Rust saved.
    @State private var editing: TrackSettings?
    @State private var peaks: [Float] = []

    private var settings: TrackSettings { editing ?? track.settings }

    var body: some View {
        ForgeCard {
            header
            WaveformView(peaks: peaks, isRecording: isRecording, audible: track.audible)
                .frame(height: 56)
                .task(id: track.clip?.path) {
                    peaks = track.clip == nil
                        ? []
                        : await model.waveform(projectID, track: track.id, buckets: 96)
                }
            HStack {
                toggle("M", label: "Mute", on: settings.muted, tint: .orange, id: "track.mute") { $0.muted.toggle() }
                toggle("S", label: "Solo", on: settings.solo, tint: .yellow, id: "track.solo") { $0.solo.toggle() }
                toggle("Echo", label: "Echo", on: settings.echo != nil, tint: .accentColor, id: "track.echo") {
                    $0.echo = $0.echo == nil ? SoundFormat.defaultEcho : nil
                }
                Spacer()
            }
            slider("Level", value: \.gainDb, in: -60...6, text: SoundFormat.gain(settings.gainDb), id: "track.gain")
            slider("Pan", value: \.pan, in: -1...1, text: SoundFormat.pan(settings.pan), id: "track.pan")
            slider("Tone", value: \.tone, in: 0...1, text: "\(Int((settings.tone * 100).rounded()))%", id: "track.tone")
        }
    }

    private var header: some View {
        HStack {
            Text(track.name)
                .font(.headline)
            if let clip = track.clip {
                Text(SoundFormat.duration(ms: clip.durationMs))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button(action: record) {
                Image(systemName: isRecording ? "record.circle.fill" : "record.circle")
                    .font(.title2)
                    .foregroundStyle(.red)
                    .symbolEffect(.pulse, isActive: isRecording)
            }
            .disabled(!canRecord)
            .accessibilityLabel(track.clip == nil ? "Record \(track.name)" : "Record \(track.name) again")
            .accessibilityIdentifier("track.record")
            Menu {
                Button("Rename", systemImage: "pencil", action: rename)
                    .accessibilityIdentifier("track.rename")
                if track.clip != nil {
                    Button("Delete Recording", systemImage: "waveform.slash", role: .destructive) {
                        model.askToClear(track, in: projectID)
                    }
                    .accessibilityIdentifier("track.clear")
                }
                Button("Remove Track", systemImage: "trash", role: .destructive) {
                    model.askToRemove(track, from: projectID)
                }
                .accessibilityIdentifier("track.remove")
            } label: {
                Image(systemName: "ellipsis.circle")
                    .font(.title3)
            }
            .disabled(!canRecord)
            .accessibilityLabel("\(track.name) options")
            .accessibilityIdentifier("track.menu")
        }
    }

    private func toggle(
        _ title: String, label: String, on: Bool, tint: Color, id: String,
        change: @escaping (inout TrackSettings) -> Void
    ) -> some View {
        Button {
            var next = settings
            change(&next)
            save(next)
        } label: {
            Text(title)
                .font(.subheadline.weight(.semibold))
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(on ? tint : Color.secondary.opacity(0.15), in: Capsule())
                .foregroundStyle(on ? Color.black : Color.primary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityValue(on ? "On" : "Off")
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityIdentifier(id)
    }

    private func slider(
        _ title: String, value: WritableKeyPath<TrackSettings, Double>,
        in range: ClosedRange<Double>, text: String, id: String
    ) -> some View {
        HStack {
            Text(title)
                .font(.subheadline)
                .frame(width: 44, alignment: .leading)
            Slider(
                value: Binding(
                    get: { settings[keyPath: value] },
                    set: { new in
                        var next = settings
                        next[keyPath: value] = new
                        editing = next
                    }
                ),
                in: range
            ) { moving in
                if !moving, let editing { save(editing) }
            }
            .accessibilityLabel(title)
            .accessibilityValue(text)
            .accessibilityIdentifier(id)
            Text(text)
                .font(.caption.monospacedDigit())
                .frame(width: 56, alignment: .trailing)
        }
    }

    private func save(_ next: TrackSettings) {
        editing = next
        Task {
            if let detail = await model.setSettings(projectID, track: track.id, next) {
                changed(detail)
            }
            editing = nil
        }
    }
}

/// A recording's loudest moments, as bars across the loop.
struct WaveformView: View {
    let peaks: [Float]
    let isRecording: Bool
    let audible: Bool

    var body: some View {
        Canvas { context, size in
            let shape = RoundedRectangle(cornerRadius: 8).path(in: CGRect(origin: .zero, size: size))
            context.fill(shape, with: .color(.secondary.opacity(0.1)))
            guard !peaks.isEmpty else { return }
            let width = size.width / CGFloat(peaks.count)
            let middle = size.height / 2
            var bars = Path()
            for (index, peak) in peaks.enumerated() {
                let height = max(1, CGFloat(peak) * (size.height - 8))
                bars.addRect(CGRect(
                    x: CGFloat(index) * width + width * 0.15,
                    y: middle - height / 2,
                    width: width * 0.7,
                    height: height
                ))
            }
            context.fill(bars, with: .color(audible ? .accentColor : .secondary))
        }
        .overlay {
            if isRecording {
                Label("Recording", systemImage: "mic.fill")
                    .font(.caption)
                    .foregroundStyle(.red)
            } else if peaks.isEmpty {
                Text("No recording yet")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement()
        .accessibilityLabel(peaks.isEmpty ? "No recording" : "Waveform")
    }
}
#endif
