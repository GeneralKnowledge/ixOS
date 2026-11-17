import Foundation
import Observation
#if canImport(AVFAudio)
    import AVFAudio
#endif

/// Where the phone's sound is going, the volume, and every audio
/// interruption and route change since launch. The audio session is the
/// process's, so while a hosted app plays (Soundscape, Pocket Library's Read
/// Aloud), its interruptions land here too: open it, take a call, come back
/// and read what iOS sent. A thin adapter over AVAudioSession; Device Lab
/// never activates the session itself.
@MainActor
@Observable
public final class AudioMonitor {
    public struct Event: Identifiable, Equatable, Sendable {
        public let id = UUID()
        public let at: Date
        public let title: String
        public let detail: String
    }

    public static let shared = AudioMonitor()

    public private(set) var outputs: [String] = []
    public private(set) var inputs: [String] = []
    public private(set) var volume: Float?
    public private(set) var otherAudioPlaying = false
    /// Newest first, at most 50.
    public private(set) var events: [Event] = []

    @ObservationIgnored private var started = false
    #if canImport(AVFAudio) && os(iOS)
        @ObservationIgnored private var observers: [NSObjectProtocol] = []
        @ObservationIgnored private var volumeObservation: NSKeyValueObservation?
    #endif

    /// Starts listening once, for the life of the process.
    public func start() {
        guard !started else { return }
        started = true
        #if canImport(AVFAudio) && os(iOS)
            let session = AVAudioSession.sharedInstance()
            let center = NotificationCenter.default
            observers.append(center.addObserver(
                forName: AVAudioSession.interruptionNotification, object: session, queue: .main
            ) { [weak self] note in
                // Only plain numbers cross into the main actor: userInfo
                // isn't Sendable.
                let info = note.userInfo ?? [:]
                let type = info[AVAudioSessionInterruptionTypeKey] as? UInt
                let reason = info[AVAudioSessionInterruptionReasonKey] as? UInt
                let options = info[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
                MainActor.assumeIsolated {
                    self?.interruption(type: type, reason: reason, options: options)
                }
            })
            observers.append(center.addObserver(
                forName: AVAudioSession.routeChangeNotification, object: session, queue: .main
            ) { [weak self] note in
                let reason = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt ?? 0
                MainActor.assumeIsolated { self?.routeChange(reason: reason) }
            })
            observers.append(center.addObserver(
                forName: AVAudioSession.mediaServicesWereResetNotification, object: session, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.log("Media services reset", "iOS restarted its audio system; apps must set up audio again.") }
            })
            volumeObservation = session.observe(\.outputVolume, options: [.new]) { [weak self] session, _ in
                let value = session.outputVolume
                Task { @MainActor in self?.volume = value }
            }
        #endif
        refresh()
    }

    /// Re-reads the route, volume and whether another app is playing.
    public func refresh() {
        #if canImport(AVFAudio) && os(iOS)
            let session = AVAudioSession.sharedInstance()
            outputs = session.currentRoute.outputs.map(Self.describe)
            inputs = session.currentRoute.inputs.map(Self.describe)
            volume = session.outputVolume
            otherAudioPlaying = session.isOtherAudioPlaying
        #endif
    }

    func log(_ title: String, _ detail: String) {
        events.insert(Event(at: Date(), title: title, detail: detail), at: 0)
        if events.count > 50 { events.removeLast(events.count - 50) }
    }

    #if canImport(AVFAudio) && os(iOS)
        private static func describe(_ port: AVAudioSessionPortDescription) -> String {
            let kind: String = switch port.portType {
            case .builtInSpeaker: "Speaker"
            case .builtInReceiver: "Earpiece"
            case .builtInMic: "Microphone"
            case .headphones: "Headphones"
            case .headsetMic: "Headset Microphone"
            case .bluetoothA2DP, .bluetoothLE, .bluetoothHFP: "Bluetooth"
            case .airPlay: "AirPlay"
            case .carAudio: "CarPlay"
            case .HDMI: "HDMI"
            case .usbAudio: "USB"
            case .lineOut: "Line Out"
            default: port.portType.rawValue
            }
            return port.portName == kind ? kind : "\(kind): \(port.portName)"
        }

        private func interruption(type raw: UInt?, reason reasonRaw: UInt?, options optionsRaw: UInt) {
            guard let raw, let type = AVAudioSession.InterruptionType(rawValue: raw) else { return }
            switch type {
            case .began:
                var detail = "Playback and recording stop."
                if let reasonRaw,
                   let reason = AVAudioSession.InterruptionReason(rawValue: reasonRaw) {
                    switch reason {
                    case .appWasSuspended: detail = "The app was suspended."
                    case .builtInMicMuted: detail = "The built-in microphone was muted."
                    case .routeDisconnected: detail = "The audio device was disconnected."
                    default: break
                    }
                }
                log("Interruption began", detail)
            case .ended:
                let resume = AVAudioSession.InterruptionOptions(rawValue: optionsRaw).contains(.shouldResume)
                log("Interruption ended", resume ? "iOS says the app may resume." : "iOS says not to resume on its own.")
            @unknown default:
                log("Interruption", "Type \(raw)")
            }
            refresh()
        }

        private func routeChange(reason raw: UInt) {
            let reason = AVAudioSession.RouteChangeReason(rawValue: raw)
            let title: String = switch reason {
            case .newDeviceAvailable: "Device connected"
            case .oldDeviceUnavailable: "Device disconnected"
            case .categoryChange: "Audio category changed"
            case .override: "Route overridden"
            case .wakeFromSleep: "Woke from sleep"
            case .noSuitableRouteForCategory: "No route for the category"
            case .routeConfigurationChange: "Route configuration changed"
            default: "Route changed"
            }
            refresh()
            log(title, "Now: " + (outputs.isEmpty ? "no output" : outputs.joined(separator: ", ")))
        }
    #endif
}
