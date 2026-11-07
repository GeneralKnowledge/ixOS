import Foundation
import WeatherBridge

/// Display helpers. Times in a report are local to the place and come as
/// "YYYY-MM-DDTHH:MM" strings, so they are formatted as text, never through
/// the device's time zone.
public enum WeatherFormat {
    public static func temperature(_ celsius: Double) -> String {
        "\(Int(celsius.rounded()))°"
    }

    /// "20:00" from "2026-11-04T20:00".
    public static func hour(_ localTime: String) -> String {
        guard let t = localTime.split(separator: "T").last, t.count >= 5 else { return localTime }
        return String(t.prefix(5))
    }

    /// "Wed" from "2026-11-04"; "Today" for the first day.
    public static func weekday(_ date: String, isFirst: Bool = false) -> String {
        if isFirst { return "Today" }
        let parser = DateFormatter()
        parser.locale = Locale(identifier: "en_US_POSIX")
        parser.timeZone = TimeZone(identifier: "UTC")
        parser.dateFormat = "yyyy-MM-dd"
        guard let day = parser.date(from: date) else { return date }
        let out = DateFormatter()
        out.timeZone = TimeZone(identifier: "UTC")
        out.setLocalizedDateFormatFromTemplate("EEE")
        return out.string(from: day)
    }

    /// "5 min", "3 h", "2 d".
    public static func age(seconds: UInt64) -> String {
        switch seconds {
        case ..<60: return "under a minute"
        case ..<3_600: return "\(seconds / 60) min"
        case ..<86_400: return "\(seconds / 3_600) h"
        default: return "\(seconds / 86_400) d"
        }
    }

    /// "NW" for 315°.
    public static func compass(_ degrees: UInt16) -> String {
        let points = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
        return points[Int((Double(degrees % 360) / 45).rounded()) % 8]
    }

    /// Text and SF Symbol for a condition.
    public static func describe(_ condition: WeatherCondition, isDay: Bool = true)
        -> (text: String, symbol: String)
    {
        switch condition {
        case .clear: return (isDay ? "Sunny" : "Clear", isDay ? "sun.max.fill" : "moon.stars.fill")
        case .mainlyClear:
            return (isDay ? "Mostly sunny" : "Mostly clear", isDay ? "sun.max.fill" : "moon.stars.fill")
        case .partlyCloudy: return ("Partly cloudy", isDay ? "cloud.sun.fill" : "cloud.moon.fill")
        case .overcast: return ("Cloudy", "cloud.fill")
        case let .fog(rime): return (rime ? "Freezing fog" : "Fog", "cloud.fog.fill")
        case let .drizzle(intensity, freezing):
            return ("\(name(intensity)) \(freezing ? "freezing drizzle" : "drizzle")", "cloud.drizzle.fill")
        case let .rain(intensity, freezing):
            return (
                "\(name(intensity)) \(freezing ? "freezing rain" : "rain")",
                freezing ? "cloud.sleet.fill" : intensity == .heavy ? "cloud.heavyrain.fill" : "cloud.rain.fill"
            )
        case let .snow(intensity): return ("\(name(intensity)) snow", "cloud.snow.fill")
        case .snowGrains: return ("Snow grains", "cloud.snow.fill")
        case let .rainShowers(intensity): return ("\(name(intensity)) showers", "cloud.sun.rain.fill")
        case let .snowShowers(intensity): return ("\(name(intensity)) snow showers", "cloud.snow.fill")
        case let .thunderstorm(hail):
            return (hail == nil ? "Thunderstorm" : "Thunderstorm with hail", "cloud.bolt.rain.fill")
        case let .unknown(code):
            return (code == UInt16.max ? "Unknown" : "Unknown (code \(code))", "questionmark.circle")
        }
    }

    /// Rain, drizzle, snow or a storm: the conditions where the chance of
    /// precipitation is worth showing under the icon.
    public static func isWet(_ condition: WeatherCondition) -> Bool {
        switch condition {
        case .drizzle, .rain, .snow, .snowGrains, .rainShowers, .snowShowers, .thunderstorm: return true
        default: return false
        }
    }

    /// "29 km/h" or "18 mph" from km/h.
    public static func speed(kmh: Double, unit: WindUnit) -> String {
        switch unit {
        case .kilometresPerHour: return "\(Int(kmh.rounded())) km/h"
        case .milesPerHour: return "\(Int((kmh / 1.609_344).rounded())) mph"
        }
    }

    /// The UV index band as the WHO names it.
    public static func uvLevel(_ index: Double) -> String {
        switch index {
        case ..<3: return "Low"
        case ..<6: return "Moderate"
        case ..<8: return "High"
        case ..<11: return "Very high"
        default: return "Extreme"
        }
    }

    /// The time now at a place `utcOffsetSeconds` from UTC, "13:14".
    public static func localClock(utcOffsetSeconds: Int32, now: Date = Date()) -> String {
        let out = DateFormatter()
        out.locale = Locale(identifier: "en_GB")
        out.timeZone = TimeZone(secondsFromGMT: Int(utcOffsetSeconds))
        out.dateFormat = "HH:mm"
        return out.string(from: now)
    }

    private static func name(_ intensity: Intensity) -> String {
        switch intensity {
        case .light: return "Light"
        case .moderate: return "Moderate"
        case .heavy: return "Heavy"
        }
    }
}
