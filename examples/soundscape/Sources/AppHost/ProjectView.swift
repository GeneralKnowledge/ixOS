#if os(iOS)
@preconcurrency import SoundscapeFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Screen 2: one project's mixer. Play loops every track in step; a
/// track's record button records one pass of the loop while the others
/// play. Each strip's settings go to Rust, which answers with the mix in
/// AVAudioEngine's units for the engine to use.
struct ProjectView: View {
    @Bindable var model: SoundscapeModel
    let projectID: UInt64
    let changes: UInt64

    @State private var detail: ProjectDetail?
    @State private var engine = LoopEngine()
    /// The metronome's click track for the project's tempo and length.
    @State private var click: URL?
    @State private var recording: UInt64?
    @State private var settingTiming = false
    @State private var tempo = 100.0
    @State private var bars = 2
    @State private var exported: Mixdown?
    @State private var renaming: Track?
    @State private var newName = ""

    var body: some View {
        Group {
            if let detail {
                mixer(detail)
            } else {
                ContentUnavailableView("Project Deleted", systemImage: "trash")
            }
        }
        .navigationTitle(detail?.summary.title ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { toolbar }
        .safeAreaInset(edge: .bottom) { transport }
        .sheet(isPresented: $settingTiming) { timingSheet }
        .sheet(item: $exported) { mixdown in
            MixdownSheet(mixdown: mixdown)
        }
        .alert("Rename Track", isPresented: Binding(
            get: { renaming != nil },
            set: { if !$0 { renaming = nil } }
        )) {
            TextField("Name", text: $newName)
                .accessibilityIdentifier("tracks.rename.name")
            Button("Rename") { renameTrack() }
                .accessibilityIdentifier("tracks.rename.save")
            Button("Cancel", role: .cancel) {}
                .accessibilityIdentifier("tracks.rename.cancel")
        }
        .forgeAlert(Binding(
            get: { settingTiming || exported != nil ? nil : model.alert },
            set: { model.alert = $0 }
        )) { action in
            Task { if let changed = await model.handle(action) { show(changed) } }
        }
        .forgeFeedback(model.feedback)
        .onAppear { show(model.detail(projectID)) }
        .onDisappear { engine.stop() }
        .onChange(of: changes) {
            if recording == nil { show(model.detail(projectID)) }
        }
        // A new click track when the tempo or length changes.
        .task(id: clickKey) {
            guard clickKey != nil else { return }
            click = await model.clickTrack(projectID, sampleRate: 48_000)
            if let detail { engine.load(detail, click: click) }
        }
    }

    private var clickKey: String? {
        detail.map { "\($0.summary.tempoBpm)-\($0.summary.bars)" }
    }

    private func mixer(_ detail: ProjectDetail) -> some View {
        ScrollView {
            VStack(spacing: 16) {
                ForEach(detail.tracks) { track in
                    TrackStrip(
                        model: model,
                        projectID: projectID,
                        track: track,
                        isRecording: recording == track.id,
                        canRecord: recording == nil,
                        changed: { show($0) },
                        record: { record(track) },
                        rename: {
                            newName = track.name
                            renaming = track
                        }
                    )
                }
                Button {
                    Task { if let changed = await model.addTrack(to: projectID) { show(changed) } }
                } label: {
                    Label("Add Track", systemImage: "plus")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .disabled(recording != nil)
                .accessibilityIdentifier("project.addTrack")
            }
            .padding()
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            Menu {
                Button("Tempo and Length", systemImage: "metronome") {
                    if let summary = detail?.summary {
                        tempo = summary.tempoBpm
                        bars = Int(summary.bars)
                    }
                    settingTiming = true
                }
                .accessibilityIdentifier("project.timing")
                Section("Mix Down to a WAV File") {
                    ForEach([1, 2, 4], id: \.self) { passes in
                        Button(passes == 1 ? "One Pass" : "\(passes) Passes", systemImage: "square.and.arrow.up") {
                            mixDown(passes)
                        }
                        .accessibilityIdentifier("project.mixdown.\(passes)")
                    }
                }
            } label: {
                Label("More", systemImage: "ellipsis.circle")
            }
            .disabled(recording != nil || model.isMixing)
            .accessibilityIdentifier("project.more")
        }
    }

    /// Play or stop, the loop's length, and mixing progress.
    private var transport: some View {
        HStack(spacing: 16) {
            Button {
                if recording != nil {
                    // Keeps the take so far; the loop pads it with silence.
                    engine.finishRecording()
                } else if engine.isPlaying {
                    engine.stop()
                } else {
                    do { try engine.play() } catch { model.report(error) }
                }
            } label: {
                Image(systemName: engine.isPlaying ? "stop.fill" : "play.fill")
                    .font(.title2)
                    .frame(width: 56, height: 56)
                    .background(Color.accentColor, in: Circle())
                    .foregroundStyle(.white)
            }
            .accessibilityLabel(engine.isPlaying ? "Stop" : "Play")
            .accessibilityIdentifier("project.play")
            Button {
                toggleMetronome()
            } label: {
                Image(systemName: "metronome")
                    .font(.title3)
                    .frame(width: 44, height: 44)
                    .foregroundStyle(metronomeOn ? Color.accentColor : .secondary)
                    .background(metronomeOn ? Color.accentColor.opacity(0.15) : .clear, in: Circle())
            }
            .accessibilityLabel("Metronome")
            .accessibilityValue(metronomeOn ? "On" : "Off")
            .accessibilityIdentifier("project.metronome")
            .disabled(recording != nil)
            VStack(alignment: .leading) {
                if model.isMixing {
                    Text("Mixing down…")
                    Button("Stop Mixing") { model.stopMixdown() }
                        .font(.footnote)
                        .accessibilityIdentifier("project.stopMixdown")
                } else if engine.isCountingIn {
                    Text("Get ready: one bar…").forgeForeground(.warning)
                } else if recording != nil {
                    Text("Recording one pass. Stop keeps it.").foregroundStyle(.red)
                } else if let reason = engine.stoppedBecause {
                    Text("Stopped. \(reason)")
                        .font(.footnote)
                        .forgeForeground(.warning)
                        .accessibilityIdentifier("project.stoppedBecause")
                } else if let summary = detail?.summary, let detail {
                    Text(SoundFormat.timing(tempo: summary.tempoBpm, bars: summary.bars))
                    Text(SoundFormat.duration(ms: Int64(detail.loopSeconds * 1000)) + " loop")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
        }
        .padding()
        .background(.bar)
    }

    private var timingSheet: some View {
        NavigationStack {
            Form {
                TimingFields(tempo: $tempo, bars: $bars)
                Section {
                    Text("Recordings stay as they are: a longer loop adds silence after them, a shorter one cuts them.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Tempo and Length")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { settingTiming = false }
                        .accessibilityIdentifier("timing.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { saveTiming() }
                        .accessibilityIdentifier("timing.save")
                }
            }
            .forgeAlert($model.alert) { _ in }
        }
        .presentationDetents([.medium])
    }

    // MARK: Actions

    private var metronomeOn: Bool { detail?.summary.metronome ?? true }

    private func show(_ changed: ProjectDetail?) {
        detail = changed
        if let changed { engine.load(changed, click: click) }
    }

    private func toggleMetronome() {
        let on = !metronomeOn
        Task { if let changed = await model.setMetronome(projectID, on: on) { show(changed) } }
    }

    private func record(_ track: Track) {
        recording = track.id
        Task {
            defer { recording = nil }
            do {
                let url = try await engine.record { rate in model.loopFrames(projectID, sampleRate: rate) }
                defer { try? FileManager.default.removeItem(at: url) }
                if let changed = await model.setClip(projectID, track: track.id, from: url) {
                    show(changed)
                }
            } catch is CancellationError {
                // Stopped from the transport.
            } catch {
                model.report(error)
            }
        }
    }

    private func mixDown(_ passes: Int) {
        Task {
            if let mixdown = await model.mixdown(projectID, passes: passes) {
                exported = mixdown
            }
        }
    }

    private func saveTiming() {
        Task {
            do {
                show(try await model.setTiming(projectID, tempo: tempo, bars: bars))
                settingTiming = false
            } catch {
                model.report(error)
            }
        }
    }

    private func renameTrack() {
        guard let track = renaming else { return }
        let name = newName
        Task {
            do {
                show(try await model.renameTrack(projectID, track: track.id, to: name))
            } catch {
                model.report(error)
            }
        }
    }
}

/// What a mixdown made, with a button to share it.
private struct MixdownSheet: View {
    let mixdown: Mixdown
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                LabeledContent("Length", value: SoundFormat.duration(ms: mixdown.durationMs))
                LabeledContent("Peak", value: SoundFormat.peak(mixdown.peakDb))
                if mixdown.limited {
                    Label("The mix was louder than a WAV file can hold, so it was turned down to fit.", systemImage: "speaker.wave.3")
                        .font(.footnote)
                }
                ShareLink(item: URL(fileURLWithPath: mixdown.path)) {
                    Label("Share WAV File", systemImage: "square.and.arrow.up")
                }
                .accessibilityIdentifier("mixdown.share")
            }
            .navigationTitle("Mixdown")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("mixdown.done")
                }
            }
        }
        .presentationDetents([.medium])
    }
}

extension Mixdown: Identifiable {
    public var id: String { path }
}
#endif
