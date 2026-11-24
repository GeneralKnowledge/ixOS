import ForgeUI
import XCTest

/// The material table and the motion vocabulary are plain values, so the
/// rules behind them are checked here on Linux (docs/forgeui.md, materials).
final class ForgeMaterialMotionTests: XCTestCase {
    func testGlassIsOnlyForControls() {
        for kind in ForgeSurfaceKind.allCases {
            let material = ForgeMaterial.material(for: kind)
            let isGlass: Bool
            if case .glass = material { isGlass = true } else { isGlass = false }
            let isControl = kind == .floatingControl || kind == .controlOverMedia
            XCTAssertEqual(isGlass, isControl, "\(kind) is \(material)")
        }
    }

    func testPassiveNoticesUseTheStandardMaterial() {
        XCTAssertEqual(ForgeMaterial.material(for: .notice), .standard)
        XCTAssertEqual(ForgeMaterial.material(for: .content), .solid)
        XCTAssertEqual(ForgeMaterial.material(for: .controlOverMedia), .glass(.clear))
    }

    func testReduceTransparencyMakesEverythingSolid() {
        for kind in ForgeSurfaceKind.allCases {
            XCTAssertEqual(ForgeMaterial.reducedTransparency(ForgeMaterial.material(for: kind)), .solid)
        }
    }

    func testFallbackNumbers() {
        XCTAssertEqual(ForgeMaterial.fallbackShadow, ForgeShadow(opacity: 0.12, radius: 12, y: 4))
        XCTAssertEqual(ForgeMaterial.clearGlassFallbackWash, 0.14)
    }

    func testMotionDurationsAscendAndOnlyEmphasisedSprings() {
        let durations = ForgeMotion.allCases.map(\.duration)
        XCTAssertEqual(durations, durations.sorted())
        XCTAssertEqual(ForgeMotion.allCases.filter(\.isSpring), [.emphasised])
        XCTAssertEqual(ForgeMotion.emphasised.damping, 0.8)
        XCTAssertEqual(ForgeMotion.ambient.duration, 0.6)
    }

    func testReduceMotionTurnsMotionOff() {
        XCTAssertFalse(ForgeMotion.animates(reduceMotion: true))
        XCTAssertTrue(ForgeMotion.animates(reduceMotion: false))
    }

    func testRadiusGainsXLWithoutRescaling() {
        let radius = ForgeRadius.standard
        XCTAssertEqual([radius.s, radius.m, radius.l, radius.xl], [7, 12, 18, 28])
    }
}
