@preconcurrency import FieldKitFFI
import MapKit
import SwiftUI

/// A trip's track as a line on Apple Maps, with its placed notes as pins.
/// The map needs a connection to draw; the track itself is recorded
/// without one (FieldKit's maps decision: online maps, offline recording).
struct TrackMap: View {
    let track: [TrackPoint]
    var notes: [Note] = []
    /// Keep the newest point in view (while recording).
    var follow = false

    @State private var position: MapCameraPosition = .automatic

    private var coordinates: [CLLocationCoordinate2D] {
        track.map { CLLocationCoordinate2D(latitude: $0.latitude, longitude: $0.longitude) }
    }

    var body: some View {
        Map(position: $position) {
            if coordinates.count > 1 {
                MapPolyline(coordinates: coordinates)
                    .stroke(.tint, lineWidth: 5)
            }
            if let start = coordinates.first {
                Marker("Start", systemImage: "flag", coordinate: start)
                    .tint(.green)
            }
            ForEach(notes) { note in
                if let at = note.at {
                    Marker(
                        note.mapTitle, systemImage: note.symbol,
                        coordinate: CLLocationCoordinate2D(latitude: at.latitude, longitude: at.longitude)
                    )
                    .tint(.orange)
                }
            }
            if follow {
                UserAnnotation()
            }
        }
        .mapControls {
            MapCompass()
            MapScaleView()
        }
        .onChange(of: track.count) {
            if follow { position = .userLocation(fallback: .automatic) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(track.isEmpty ? "Map, no track yet" : "Map of the track")
        .accessibilityIdentifier("trip.map")
    }
}

extension Note {
    var mapTitle: String {
        switch kind {
        case .text(let body): return String(body.prefix(40))
        case .photo(_, let caption): return caption.isEmpty ? "Photo" : caption
        case .voice: return "Voice note"
        }
    }

    var symbol: String {
        switch kind {
        case .text: return "note.text"
        case .photo: return "camera"
        case .voice: return "waveform"
        }
    }
}
