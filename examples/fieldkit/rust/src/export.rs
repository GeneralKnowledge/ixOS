//! A trip as a file other apps read: GPX 1.1 (maps, fitness and hiking
//! apps) or GeoJSON (GIS tools, web maps). Pure functions, so they test on
//! Linux.

use crate::model::{Note, NoteKind, TripSummary};
use crate::track::TrackPoint;

/// The file formats a trip exports to.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Gpx,
    GeoJson,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gpx => "gpx",
            Self::GeoJson => "geojson",
        }
    }
}

/// What a note is called in a file: its text, or what kind it is.
fn note_name(note: &Note) -> String {
    match &note.kind {
        NoteKind::Text { body } => body.clone(),
        NoteKind::Photo { caption, .. } if !caption.is_empty() => caption.clone(),
        NoteKind::Photo { .. } => "Photo".into(),
        NoteKind::Voice { .. } => "Voice note".into(),
    }
}

fn note_type(note: &Note) -> &'static str {
    match note.kind {
        NoteKind::Text { .. } => "text",
        NoteKind::Photo { .. } => "photo",
        NoteKind::Voice { .. } => "voice",
    }
}

/// GPX 1.1: the track as one segment, and each placed note as a waypoint.
pub fn gpx(trip: &TripSummary, points: &[TrackPoint], notes: &[Note]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <gpx version=\"1.1\" creator=\"FieldKit\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
    );
    out.push_str(&format!(
        "  <metadata>\n    <name>{}</name>\n    <time>{}</time>\n  </metadata>\n",
        xml(&trip.title),
        iso8601(trip.started_ms)
    ));
    for note in notes {
        let Some(at) = note.at else { continue };
        out.push_str(&format!(
            "  <wpt lat=\"{}\" lon=\"{}\">\n    <time>{}</time>\n    <name>{}</name>\n    <type>{}</type>\n  </wpt>\n",
            at.latitude,
            at.longitude,
            iso8601(note.created_ms),
            xml(&note_name(note)),
            note_type(note)
        ));
    }
    out.push_str(&format!(
        "  <trk>\n    <name>{}</name>\n    <trkseg>\n",
        xml(&trip.title)
    ));
    for p in points {
        out.push_str(&format!(
            "      <trkpt lat=\"{}\" lon=\"{}\">",
            p.latitude, p.longitude
        ));
        if let Some(ele) = p.altitude_m {
            out.push_str(&format!("<ele>{ele}</ele>"));
        }
        out.push_str(&format!(
            "<time>{}</time></trkpt>\n",
            iso8601(p.timestamp_ms)
        ));
    }
    out.push_str("    </trkseg>\n  </trk>\n</gpx>\n");
    out
}

/// GeoJSON (RFC 7946): a FeatureCollection with the track as a LineString
/// and each placed note as a Point. Positions are `[longitude, latitude]`,
/// with the altitude third when every point has one.
pub fn geojson(trip: &TripSummary, points: &[TrackPoint], notes: &[Note]) -> String {
    let with_altitude = !points.is_empty() && points.iter().all(|p| p.altitude_m.is_some());
    let coordinates: Vec<serde_json::Value> = points
        .iter()
        .map(|p| match p.altitude_m {
            Some(alt) if with_altitude => serde_json::json!([p.longitude, p.latitude, alt]),
            _ => serde_json::json!([p.longitude, p.latitude]),
        })
        .collect();
    let mut features = vec![serde_json::json!({
        "type": "Feature",
        "geometry": { "type": "LineString", "coordinates": coordinates },
        "properties": {
            "name": trip.title,
            "started": iso8601(trip.started_ms),
            "ended": trip.ended_ms.map(iso8601),
            "distance_m": trip.distance_m,
            "climb_m": trip.climb_m,
            "times": points.iter().map(|p| iso8601(p.timestamp_ms)).collect::<Vec<_>>(),
        }
    })];
    for note in notes {
        let Some(at) = note.at else { continue };
        features.push(serde_json::json!({
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [at.longitude, at.latitude] },
            "properties": {
                "name": note_name(note),
                "kind": note_type(note),
                "time": iso8601(note.created_ms),
            }
        }));
    }
    let collection = serde_json::json!({ "type": "FeatureCollection", "features": features });
    format!("{collection:#}\n")
}

/// Escapes text for XML content and attributes.
fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 forbids most control characters, even escaped.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// UTC time as `2026-11-11T17:33:23Z` (milliseconds dropped).
pub fn iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Position;

    fn trip() -> TripSummary {
        TripSummary {
            id: 1,
            title: "Fish & chips <run>".into(),
            started_ms: 1_794_416_000_000,
            ended_ms: Some(1_794_419_600_000),
            recording: false,
            distance_m: 22.2,
            climb_m: 0.0,
            point_count: 2,
            note_count: 1,
        }
    }

    fn points() -> Vec<TrackPoint> {
        vec![
            TrackPoint {
                latitude: 51.5,
                longitude: -0.12,
                altitude_m: Some(20.0),
                horizontal_accuracy_m: 5.0,
                timestamp_ms: 1_794_416_000_000,
            },
            TrackPoint {
                latitude: 51.5002,
                longitude: -0.12,
                altitude_m: Some(21.5),
                horizontal_accuracy_m: 5.0,
                timestamp_ms: 1_794_416_010_000,
            },
        ]
    }

    fn notes() -> Vec<Note> {
        vec![
            Note {
                id: 1,
                trip_id: 1,
                created_ms: 1_794_416_005_000,
                at: Some(Position {
                    latitude: 51.5001,
                    longitude: -0.12,
                }),
                kind: NoteKind::Text {
                    body: "Gate \"B\"".into(),
                },
            },
            // Not placed: left out of both files.
            Note {
                id: 2,
                trip_id: 1,
                created_ms: 1_794_416_006_000,
                at: None,
                kind: NoteKind::Voice {
                    path: "/x.m4a".into(),
                    duration_ms: 1000,
                },
            },
        ]
    }

    #[test]
    fn dates_are_utc() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601(1_794_418_403_999), "2026-11-11T17:33:23Z");
        assert_eq!(iso8601(-1000), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn gpx_has_the_track_and_placed_notes_escaped() {
        let gpx = gpx(&trip(), &points(), &notes());
        assert!(gpx.contains("<name>Fish &amp; chips &lt;run&gt;</name>"));
        assert!(gpx.contains(
            "<trkpt lat=\"51.5002\" lon=\"-0.12\"><ele>21.5</ele><time>2026-11-11T16:53:30Z</time></trkpt>"
        ));
        assert!(gpx.contains("<name>Gate &quot;B&quot;</name>"));
        assert_eq!(gpx.matches("<wpt ").count(), 1);
        assert_eq!(gpx.matches("<trkpt ").count(), 2);
        assert!(gpx.ends_with("</gpx>\n"));
    }

    #[test]
    fn geojson_is_longitude_first() {
        let text = geojson(&trip(), &points(), &notes());
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let features = value["features"].as_array().unwrap();
        assert_eq!(features.len(), 2);
        assert_eq!(
            features[0]["geometry"]["coordinates"][1],
            serde_json::json!([-0.12, 51.5002, 21.5])
        );
        assert_eq!(features[0]["properties"]["name"], "Fish & chips <run>");
        assert_eq!(features[1]["geometry"]["type"], "Point");
        assert_eq!(features[1]["properties"]["kind"], "text");
    }

    #[test]
    fn geojson_drops_altitude_unless_every_point_has_one() {
        let mut points = points();
        points[0].altitude_m = None;
        let value: serde_json::Value =
            serde_json::from_str(&geojson(&trip(), &points, &[])).unwrap();
        assert_eq!(
            value["features"][0]["geometry"]["coordinates"][1],
            serde_json::json!([-0.12, 51.5002])
        );
    }
}
