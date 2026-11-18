import Foundation

/// The geometry of Spatial Sensor Lab's 3D view: a phone-shaped box turned by
/// the phone's attitude, seen from a camera fixed in the world (south of the
/// phone and above it, looking north and down). Plain math, so it is tested
/// without SwiftUI; `SensorLabView` draws what it returns with a Canvas
/// (no SceneKit or Metal, so it builds with xtool on Linux).
///
/// Points come back in unit coordinates around the centre (x right, y down);
/// the view multiplies them by its size.
public struct SensorLabScene: Sendable {
    /// The camera's angle above the horizon.
    public var elevation: Double = 28 * .pi / 180
    /// How far the camera stands from the phone, in phone half-heights.
    public var distance: Double = 7

    public init() {}

    /// Half the phone's width, height and thickness, in phone half-heights.
    public static let halfSize = SensorVector(0.48, 1.0, 0.06)

    // Camera axes in world terms (x north, y west, z up).
    private var right: SensorVector { SensorVector(0, -1, 0) }
    private var forward: SensorVector { SensorVector(cos(elevation), 0, -sin(elevation)) }
    private var up: SensorVector { SensorVector(sin(elevation), 0, cos(elevation)) }

    public struct Projected: Equatable, Sendable {
        public var x: Double
        public var y: Double
        /// Distance in front of the phone's centre, away from the camera:
        /// positive is behind it.
        public var depth: Double
    }

    /// A world point on screen.
    public func project(_ p: SensorVector) -> Projected {
        let depth = p.dot(forward)
        let scale = distance / (distance + depth)
        return Projected(x: p.dot(right) * scale, y: -p.dot(up) * scale, depth: depth)
    }

    public enum FaceKind: Sendable {
        case screen, back, edge
    }

    public struct Face: Sendable {
        public var kind: FaceKind
        public var corners: [Projected]
        /// 0 (in shadow) to 1 (facing the light).
        public var light: Double
    }

    /// The faces of the phone the camera can see. The box is convex, so no
    /// visible face covers another and their order doesn't matter.
    public func phoneFaces(_ rotation: SensorRotation) -> [Face] {
        let h = Self.halfSize
        let light = SensorVector(0.3, 0.5, 0.81)  // from the upper left
        // Each face: its outward normal and its four corners, device axes.
        let faces: [(FaceKind, SensorVector, [SensorVector])] = [
            (.screen, SensorVector(0, 0, 1), rectangle(z: h.z, h)),
            (.back, SensorVector(0, 0, -1), rectangle(z: -h.z, h)),
            (.edge, SensorVector(1, 0, 0), [
                SensorVector(h.x, -h.y, -h.z), SensorVector(h.x, h.y, -h.z),
                SensorVector(h.x, h.y, h.z), SensorVector(h.x, -h.y, h.z),
            ]),
            (.edge, SensorVector(-1, 0, 0), [
                SensorVector(-h.x, -h.y, -h.z), SensorVector(-h.x, h.y, -h.z),
                SensorVector(-h.x, h.y, h.z), SensorVector(-h.x, -h.y, h.z),
            ]),
            (.edge, SensorVector(0, 1, 0), [
                SensorVector(-h.x, h.y, -h.z), SensorVector(h.x, h.y, -h.z),
                SensorVector(h.x, h.y, h.z), SensorVector(-h.x, h.y, h.z),
            ]),
            (.edge, SensorVector(0, -1, 0), [
                SensorVector(-h.x, -h.y, -h.z), SensorVector(h.x, -h.y, -h.z),
                SensorVector(h.x, -h.y, h.z), SensorVector(-h.x, -h.y, h.z),
            ]),
        ]
        return faces.compactMap { kind, normal, corners in
            let n = rotation.toWorld(normal)
            guard isFacingCamera(n, at: rotation.toWorld(normal * abs(normal.dot(h)))) else {
                return nil
            }
            return Face(
                kind: kind,
                corners: corners.map { project(rotation.toWorld($0)) },
                light: 0.35 + 0.65 * max(0, n.dot(light)))
        }
    }

    /// A rectangle on the screen or back plane, device axes, inset from the
    /// edges by `inset`.
    public func rectangle(z: Double, _ h: SensorVector, inset: Double = 0) -> [SensorVector] {
        let x = h.x - inset
        let y = h.y - inset
        return [SensorVector(-x, -y, z), SensorVector(x, -y, z), SensorVector(x, y, z), SensorVector(-x, y, z)]
    }

    /// A flat shape on the phone (the display, the camera), projected, or
    /// nil when its side faces away from the camera.
    public func detail(_ corners: [SensorVector], normal: SensorVector, rotation: SensorRotation) -> [Projected]? {
        let n = rotation.toWorld(normal)
        guard let first = corners.first, isFacingCamera(n, at: rotation.toWorld(first)) else { return nil }
        return corners.map { project(rotation.toWorld($0)) }
    }

    /// Whether a surface with world normal `n` through world point `p`
    /// faces the camera, which sits at `-forward * distance`.
    private func isFacingCamera(_ n: SensorVector, at p: SensorVector) -> Bool {
        let camera = forward * -distance
        return n.dot(camera - p) > 0
    }

    /// A circle on the horizontal plane `z` below the phone, for the
    /// compass ring.
    public func ring(radius: Double, z: Double, segments: Int = 72) -> [Projected] {
        (0...segments).map { i in
            let a = Double(i) / Double(segments) * 2 * .pi
            return project(SensorVector(radius * cos(a), radius * sin(a), z))
        }
    }

    /// An arrow from the centre along a world direction, `length` long.
    public func arrow(_ direction: SensorVector, length: Double) -> (tail: Projected, tip: Projected)? {
        let l = direction.length
        guard l > 0 else { return nil }
        return (project(.zero), project(direction * (length / l)))
    }

    /// The field arrow's length for a strength: Earth's usual 25-65 µT reads
    /// as about one to two phone half-heights, a magnet longer, up to 3.
    public static func arrowLength(microtesla: Double) -> Double {
        min(max(microtesla / 35, 0.15), 3)
    }
}
