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
        case .clear: return ("Clear", isDay ? "sun.max.fill" : "moon.stars.fill")
        case .mainlyClear: return ("Mainly clear", isDay ? "sun.min.fill" : "moon.fill")
        case .partlyCloudy: return ("Partly cloudy", isDay ? "cloud.sun.fill" : "cloud.moon.fill")
        case .overcast: return ("Overcast", "cloud.fill")
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

    private static func name(_ intensity: Intensity) -> String {
        switch intensity {
        case .light: return "Light"
        case .moderate: return "Moderate"
        case .heavy: return "Heavy"
        }
    }
}
