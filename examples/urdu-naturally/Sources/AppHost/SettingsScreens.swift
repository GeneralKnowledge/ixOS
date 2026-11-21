@preconcurrency import AVFoundation
@preconcurrency import UrduNaturallyFFI
import ForgeUI
import Speech
import SwiftUI

/// Your name, how much English to show, the speech check, starting over.
struct SettingsScreen: View {
    let model: UrduModel
    @State private var name = ""
    @State private var confirmStartOver = false

    var body: some View {
        Form {
            Section {
                TextField("Your name", text: $name)
                    .autocorrectionDisabled()
                    .submitLabel(.done)
                    .onSubmit { Task { await model.setName(name) } }
                    .accessibilityIdentifier("settings.name")
            } header: {
                Text("Your name")
            } footer: {
                Text("For \u{201C}Mera naam … hai\u{201D}. You learn the forms a man uses when he talks about himself.")
            }

            Section {
                Picker("English", selection: Binding(
                    get: { model.settings.english },
                    set: { value in Task { await model.setEnglish(value) } }
                )) {
                    ForEach(EnglishSupport.all, id: \.self) { option in
                        Text(option.label).tag(option)
                    }
                }
                .accessibilityIdentifier("settings.english")
            } header: {
                Text("Translations")
            } footer: {
                Text("Fading shows English for new phrases, smaller once they're familiar, and behind a tap once you understand them. A tap always shows it.")
            }

            Section {
                NavigationLink(value: Route.speechCheck) {
                    Label("Speech check", systemImage: "waveform")
                }
                .accessibilityIdentifier("settings.speechCheck")
            } footer: {
                Text("Which Urdu voices this iPhone has, and whether it can recognise spoken Urdu.")
            }

            Section {
                Button("Start over", role: .destructive) { confirmStartOver = true }
                    .accessibilityIdentifier("settings.startOver")
            } footer: {
                Text("All the Urdu in this version is a draft: none of it has been checked by a native speaker yet.")
            }
        }
        .navigationTitle("Settings")
        .onAppear { name = model.settings.name }
        .onDisappear {
            if name != model.settings.name { Task { await model.setName(name) } }
        }
        .confirmationDialog("Forget all your progress?", isPresented: $confirmStartOver, titleVisibility: .visible) {
            Button("Start Over", role: .destructive) { Task { await model.startOver() } }
                .accessibilityIdentifier("settings.startOver.confirm")
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Every phrase goes back to new. Your name and settings stay.")
        }
    }
}

/// The early speech benchmark, on the phone itself: which Urdu voices are
/// installed (with a sample to judge them by ear), and whether Apple's
/// speech recogniser supports Urdu. It reports what the phone says; it
/// judges nothing.
struct SpeechCheckScreen: View {
    private static let sample = "کیا حال ہے؟ میں ٹھیک ہوں، تم سناؤ؟"
    @State private var recognisers: [String] = []
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        List {
            Section {
                if Narrator.urduVoices.isEmpty {
                    Text("No Urdu voice is installed. Settings, Accessibility, Spoken Content, Voices may offer one to download.")
                } else {
                    ForEach(Narrator.urduVoices, id: \.identifier) { voice in
                        Button {
                            Narrator.shared.stop()
                            let synth = SampleSpeaker.shared
                            synth.say(Self.sample, voice: voice)
                        } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(voice.name).foregroundStyle(theme.colors.textPrimary)
                                Text("\(voice.language) · \(gender(voice)) · \(quality(voice))")
                                    .font(.footnote)
                                    .foregroundStyle(theme.colors.textSecondary)
                            }
                        }
                        .accessibilityIdentifier("speechCheck.voice")
                    }
                }
            } header: {
                Text("Urdu voices")
            } footer: {
                Text("Tap a voice to hear \u{201C}Kya haal hai? Main theek hoon, tum sunao?\u{201D} \(pairNote)")
            }

            Section {
                if recognisers.isEmpty {
                    Text("This iPhone's speech recogniser lists no Urdu.")
                } else {
                    ForEach(recognisers, id: \.self) { Text($0) }
                }
            } header: {
                Text("Speech recognition")
            } footer: {
                Text("Recognising what you said is not the same as judging your pronunciation; the app won't pretend it is.")
            }
        }
        .navigationTitle("Speech check")
        .onAppear {
            recognisers = SFSpeechRecognizer.supportedLocales()
                .map(\.identifier)
                .filter { $0.hasPrefix("ur") }
                .sorted()
        }
    }

    /// Whether the app can alternate a woman's and a man's voice.
    private var pairNote: String {
        switch (Narrator.hasVoice(of: .female), Narrator.hasVoice(of: .male)) {
        case (true, true): "There's a woman's and a man's voice, so Listen alternates them."
        case (true, false): "There's no man's voice, so men's lines are read by a woman's voice."
        case (false, true): "There's no woman's voice, so women's lines are read by a man's voice."
        case (false, false): "No voice says whether it's a man's or a woman's, so one voice reads every line."
        }
    }

    private func gender(_ voice: AVSpeechSynthesisVoice) -> String {
        switch voice.gender {
        case .male: "male"
        case .female: "female"
        default: "unspecified"
        }
    }

    private func quality(_ voice: AVSpeechSynthesisVoice) -> String {
        switch voice.quality {
        case .premium: "premium"
        case .enhanced: "enhanced"
        default: "default"
        }
    }
}

/// Plays the speech check's sample in one chosen voice.
@MainActor
private final class SampleSpeaker {
    static let shared = SampleSpeaker()
    private let synthesizer = AVSpeechSynthesizer()

    func say(_ text: String, voice: AVSpeechSynthesisVoice) {
        synthesizer.stopSpeaking(at: .immediate)
        try? ForgeAudioSession.activate(.playback)
        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = voice
        synthesizer.speak(utterance)
    }
}
