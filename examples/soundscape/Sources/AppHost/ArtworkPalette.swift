// The colours Now Playing is drawn in, taken from the playing song's
// artwork. Foundation only, so the rules are tested on Linux; the view reads
// the pixels (NowPlayingBackground.swift).

import Foundation

/// A colour as three channels from 0 to 1.
public struct PaletteColor: Equatable, Sendable {
    public var red: Double
    public var green: Double
    public var blue: Double

    public init(red: Double, green: Double, blue: Double) {
        self.red = red
        self.green = green
        self.blue = blue
    }

    /// Relative luminance (WCAG 2), 0 for black to 1 for white.
    public var luminance: Double {
        func channel(_ c: Double) -> Double {
            c <= 0.03928 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    /// Contrast ratio of white text over this colour.
    public var whiteContrast: Double { whiteContrast(atOpacity: 1) }

    /// Contrast ratio of white text drawn at `opacity` over this colour:
    /// the text's colour is white blended with this one, channel by channel,
    /// as the screen composites it.
    public func whiteContrast(atOpacity opacity: Double) -> Double {
        let text = PaletteColor(
            red: opacity + (1 - opacity) * red,
            green: opacity + (1 - opacity) * green,
            blue: opacity + (1 - opacity) * blue
        )
        return (text.luminance + 0.05) / (luminance + 0.05)
    }

    /// Saturation and value, HSV style.
    var saturation: Double {
        let maxC = max(red, green, blue)
        return maxC == 0 ? 0 : (maxC - min(red, green, blue)) / maxC
    }

    /// This colour scaled darker until white text over it, drawn at
    /// `opacity`, reaches `ratio`.
    func darkened(toWhiteContrast ratio: Double, atOpacity opacity: Double = 1) -> PaletteColor {
        var color = self
        var steps = 0
        while color.whiteContrast(atOpacity: opacity) < ratio && steps < 60 {
            color = PaletteColor(red: color.red * 0.92, green: color.green * 0.92, blue: color.blue * 0.92)
            steps += 1
        }
        return color
    }

    /// A colour from hue, saturation and brightness, each 0 to 1.
    public static func hsb(_ hue: Double, _ saturation: Double, _ brightness: Double) -> PaletteColor {
        let h = (hue - hue.rounded(.down)) * 6
        let i = Int(h) % 6
        let f = h - Double(Int(h))
        let p = brightness * (1 - saturation)
        let q = brightness * (1 - saturation * f)
        let t = brightness * (1 - saturation * (1 - f))
        switch i {
        case 0: return PaletteColor(red: brightness, green: t, blue: p)
        case 1: return PaletteColor(red: q, green: brightness, blue: p)
        case 2: return PaletteColor(red: p, green: brightness, blue: t)
        case 3: return PaletteColor(red: p, green: q, blue: brightness)
        case 4: return PaletteColor(red: t, green: p, blue: brightness)
        default: return PaletteColor(red: brightness, green: p, blue: q)
        }
    }
}

/// Now Playing's background: a lighter colour at the top and a darker one
/// at the bottom, both dark enough for white text, including the secondary
/// text drawn at `secondaryTextOpacity`.
public struct ArtworkPalette: Equatable, Sendable {
    public var top: PaletteColor
    public var bottom: PaletteColor

    /// White text needs at least this contrast over the lighter stop.
    public static let minimumContrast = 4.5

    /// The opacity of Now Playing's secondary text (the artist line, the
    /// radio row) over the palette. The palette is darkened until text at
    /// this opacity, not only opaque white, reaches `minimumContrast`.
    public static let secondaryTextOpacity = 0.75

    /// The palette of a song with no artwork: from the hue its stand-in
    /// (the coloured initial) is drawn in, so the player and the stand-in
    /// match.
    public static func standIn(title: String) -> ArtworkPalette {
        let hue = standInHue(title)
        return ArtworkPalette(
            top: PaletteColor.hsb(hue, 0.6, 0.55)
                .darkened(toWhiteContrast: minimumContrast, atOpacity: secondaryTextOpacity),
            bottom: PaletteColor.hsb(hue, 0.75, 0.2)
        )
    }

    /// The hue `Artwork` draws a title's stand-in in, 0 to 1.
    public static func standInHue(_ title: String) -> Double {
        Double(title.unicodeScalars.reduce(0) { ($0 &* 31 &+ Int($1.value)) & 0xFFFF } % 360) / 360
    }

    /// The palette of an image, from its pixels as RGBA bytes (any small
    /// size; 24 by 24 is plenty). Nil when there are no opaque pixels.
    ///
    /// The top is the most colourful part of the picture, so a dark cover
    /// with one red light still gives a red player; the bottom is the
    /// picture's average, darker. Both are darkened until white text reads.
    public static func from(rgba bytes: [UInt8]) -> ArtworkPalette? {
        var sum = (r: 0.0, g: 0.0, b: 0.0)
        var count = 0.0
        var vivid: PaletteColor?
        var vividScore = -1.0
        var index = 0
        while index + 3 < bytes.count {
            let alpha = Double(bytes[index + 3]) / 255
            if alpha > 0.5 {
                // Bytes are premultiplied by alpha in a CGContext; undo it.
                let r = min(Double(bytes[index]) / 255 / alpha, 1)
                let g = min(Double(bytes[index + 1]) / 255 / alpha, 1)
                let b = min(Double(bytes[index + 2]) / 255 / alpha, 1)
                sum.r += r
                sum.g += g
                sum.b += b
                count += 1
                let color = PaletteColor(red: r, green: g, blue: b)
                // Colourful and not too dark or too bright.
                let value = max(r, g, b)
                let score = color.saturation * (1 - abs(value - 0.65))
                if score > vividScore {
                    vividScore = score
                    vivid = color
                }
            }
            index += 4
        }
        guard count > 0 else { return nil }
        let average = PaletteColor(red: sum.r / count, green: sum.g / count, blue: sum.b / count)
        // A grey picture has no colour to lead with: use its average.
        let lead = (vivid?.saturation ?? 0) > 0.25 ? vivid! : average
        return ArtworkPalette(
            top: lead.darkened(toWhiteContrast: minimumContrast, atOpacity: secondaryTextOpacity),
            bottom: PaletteColor(red: average.red * 0.35, green: average.green * 0.35, blue: average.blue * 0.35)
        )
    }
}
