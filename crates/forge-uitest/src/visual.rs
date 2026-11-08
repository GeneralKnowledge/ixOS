//! Visual regression: compare a screenshot with an approved baseline and say
//! what changed, where, and which elements are there.
//!
//! Pixels alone tell an agent "something moved"; the accessibility tree saved
//! with each screenshot tells it what: an element that disappeared, a label
//! that changed, a control that became disabled, or a new accessibility
//! problem. Both are compared here, on Linux, with no phone.
//!
//! The pixel rule (docs/visual-regression.md):
//! - a pixel changed when any colour channel differs by more than
//!   `tolerance` (default 16 of 255);
//! - changed pixels are grouped into regions (4-pixel cells, touching cells
//!   join);
//! - a region counts when it is at least `min_region_pt` points (default 6)
//!   in both directions, or at least `line_pt` (default 40) in one. That
//!   keeps a blinking text cursor (2 pt wide, about 20 pt tall) out and a
//!   changed word or a vanished divider line in;
//! - masked areas (the status bar by default, and elements the scenario
//!   names) are never compared.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::tree::{Rect, UiNode};

/// An 8-bit RGBA image.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: [u8; 4]) -> Image {
        let rgba = rgba
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect();
        Image {
            width,
            height,
            rgba,
        }
    }

    fn at(&self, x: u32, y: u32) -> &[u8] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        &self.rgba[i..i + 4]
    }

    fn put(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba[i..i + 4].copy_from_slice(&px);
    }

    /// Fills a rectangle, clipped to the image. For tests and drawing.
    pub fn fill(&mut self, rect: PixelRect, px: [u8; 4]) {
        let rect = rect.clip(self.width, self.height);
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                self.put(x, y, px);
            }
        }
    }
}

/// Decodes a PNG (any 8- or 16-bit colour type) to RGBA.
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("PNG: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG: {e}"))?;
    let pixels = info.width as usize * info.height as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => {
            buf.truncate(pixels * 4);
            buf
        }
        png::ColorType::Rgb => buf[..pixels * 3]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf[..pixels].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..pixels * 2]
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err("PNG: palette was not expanded".into()),
    };
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

/// Width and height from a PNG's header, without decoding the pixels.
pub fn png_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    let reader = png::Decoder::new(bytes)
        .read_info()
        .map_err(|e| format!("PNG: {e}"))?;
    let info = reader.info();
    Ok((info.width, info.height))
}

pub fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&image.rgba)
            .map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// A rectangle in screenshot pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    fn clip(self, width: u32, height: u32) -> PixelRect {
        let x = self.x.min(width);
        let y = self.y.min(height);
        PixelRect {
            x,
            y,
            width: self.width.min(width - x),
            height: self.height.min(height - y),
        }
    }

    /// A frame in points, at `scale` pixels per point, rounded outwards.
    pub fn from_points(frame: Rect, scale: f64) -> PixelRect {
        let x0 = (frame.x * scale).floor().max(0.0);
        let y0 = (frame.y * scale).floor().max(0.0);
        let x1 = ((frame.x + frame.width) * scale).ceil().max(x0);
        let y1 = ((frame.y + frame.height) * scale).ceil().max(y0);
        PixelRect {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        }
    }

    fn to_points(self, scale: f64) -> Rect {
        let r = |v: u32| (v as f64 / scale * 10.0).round() / 10.0;
        Rect {
            x: r(self.x),
            y: r(self.y),
            width: r(self.width),
            height: r(self.height),
        }
    }
}

/// How strictly to compare.
#[derive(Debug, Clone)]
pub struct Options {
    /// Largest per-channel difference that still counts as the same colour.
    pub tolerance: u8,
    /// A region counts when it is at least this many points in both directions…
    pub min_region_pt: f64,
    /// …or at least this many points in one (a line).
    pub line_pt: f64,
    /// Pixels per point (3 on most current iPhones).
    pub scale: f64,
    /// Areas never compared.
    pub masks: Vec<PixelRect>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            tolerance: 16,
            min_region_pt: 6.0,
            line_pt: 40.0,
            scale: 3.0,
            masks: Vec::new(),
        }
    }
}

/// The scale of a screenshot: its width over the app's width in points (the
/// tree's root frame), else a guess from the width (3x from 1080 px).
pub fn scale_of(image_width: u32, tree: Option<&UiNode>) -> f64 {
    tree.and_then(|t| t.frame)
        .filter(|f| f.width > 0.0)
        .map(|f| image_width as f64 / f.width)
        .filter(|s| (1.0..=4.0).contains(s))
        .unwrap_or(if image_width >= 1080 { 3.0 } else { 2.0 })
}

/// The status bar (clock, battery, signal): it changes every minute, so it
/// is masked unless a scenario says otherwise. 54 pt on Face ID iPhones
/// (screens 812 pt and taller), 20 pt on the others.
pub fn status_bar(width: u32, height: u32, scale: f64) -> PixelRect {
    let points = if height as f64 / scale >= 812.0 {
        54.0
    } else {
        20.0
    };
    PixelRect {
        x: 0,
        y: 0,
        width,
        height: ((points * scale).ceil() as u32).min(height),
    }
}

/// One area that changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Region {
    /// In points, like the tree's frames.
    pub rect: Rect,
    /// In screenshot pixels.
    pub pixels: PixelRect,
    /// How many pixels in it changed.
    pub changed: u64,
    /// The elements there, as `forge ui tree` prints them (filled in by
    /// [`Region::locate`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// The sizes differ; nothing else was compared.
    pub size_changed: bool,
    pub changed_pixels: u64,
    pub compared_pixels: u64,
    /// Regions that count, largest first.
    pub regions: Vec<Region>,
    /// Regions too small to count (cursor blinks, anti-aliasing).
    pub ignored_regions: usize,
    /// The actual screenshot, washed out, with changed pixels red, masks
    /// blue and counted regions outlined. `None` when the sizes differ.
    pub diff: Option<Image>,
}

impl Comparison {
    /// Whether anything that counts changed.
    pub fn changed(&self) -> bool {
        self.size_changed || !self.regions.is_empty()
    }

    pub fn changed_ratio(&self) -> f64 {
        if self.compared_pixels == 0 {
            return 0.0;
        }
        self.changed_pixels as f64 / self.compared_pixels as f64
    }
}

const CELL: u32 = 4;

/// Compares `actual` with `baseline`.
pub fn compare(baseline: &Image, actual: &Image, opts: &Options) -> Comparison {
    if baseline.width != actual.width || baseline.height != actual.height {
        return Comparison {
            size_changed: true,
            changed_pixels: 0,
            compared_pixels: 0,
            regions: Vec::new(),
            ignored_regions: 0,
            diff: None,
        };
    }
    let (w, h) = (actual.width, actual.height);
    let masks: Vec<PixelRect> = opts.masks.iter().map(|m| m.clip(w, h)).collect();
    let (cw, ch) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut cells = vec![0u32; cw as usize * ch as usize];
    let mut diff = Image {
        width: w,
        height: h,
        rgba: vec![0; actual.rgba.len()],
    };
    let mut changed_pixels = 0u64;
    let mut compared_pixels = 0u64;
    for y in 0..h {
        for x in 0..w {
            let a = actual.at(x, y);
            if masks.iter().any(|m| m.contains(x, y)) {
                let g = gray(a) / 2;
                diff.put(x, y, [g, g, 128 + g / 2, 255]);
                continue;
            }
            compared_pixels += 1;
            let b = baseline.at(x, y);
            let delta = a
                .iter()
                .zip(b)
                .map(|(p, q)| p.abs_diff(*q))
                .max()
                .unwrap_or(0);
            if delta > opts.tolerance {
                changed_pixels += 1;
                cells[((y / CELL) * cw + x / CELL) as usize] += 1;
                diff.put(x, y, [230, 20, 20, 255]);
            } else {
                // Washed out, so the red stands out.
                let g = 170 + gray(a) / 3;
                diff.put(x, y, [g, g, g, 255]);
            }
        }
    }

    let mut regions = Vec::new();
    let mut ignored_regions = 0;
    let mut seen = vec![false; cells.len()];
    let min_px = opts.min_region_pt * opts.scale;
    let line_px = opts.line_pt * opts.scale;
    for start in 0..cells.len() {
        if cells[start] == 0 || seen[start] {
            continue;
        }
        // Flood fill over touching cells (8 neighbours).
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        let mut count = 0u64;
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (cx, cy) = (i as u32 % cw, i as u32 / cw);
            count += cells[i] as u64;
            x0 = x0.min(cx);
            y0 = y0.min(cy);
            x1 = x1.max(cx);
            y1 = y1.max(cy);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= cw as i32 || ny >= ch as i32 {
                        continue;
                    }
                    let n = (ny as u32 * cw + nx as u32) as usize;
                    if cells[n] > 0 && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        let pixels = PixelRect {
            x: x0 * CELL,
            y: y0 * CELL,
            width: (x1 - x0 + 1) * CELL,
            height: (y1 - y0 + 1) * CELL,
        }
        .clip(w, h);
        let (rw, rh) = (pixels.width as f64, pixels.height as f64);
        if (rw >= min_px && rh >= min_px) || rw.max(rh) >= line_px {
            regions.push(Region {
                rect: pixels.to_points(opts.scale),
                pixels,
                changed: count,
                elements: Vec::new(),
            });
        } else {
            ignored_regions += 1;
        }
    }
    regions.sort_by_key(|r| std::cmp::Reverse(r.changed));
    for region in &regions {
        outline(&mut diff, region.pixels, [200, 0, 200, 255]);
    }
    Comparison {
        size_changed: false,
        changed_pixels,
        compared_pixels,
        regions,
        ignored_regions,
        diff: Some(diff),
    }
}

fn gray(px: &[u8]) -> u8 {
    ((px[0] as u32 * 3 + px[1] as u32 * 6 + px[2] as u32) / 10) as u8
}

fn outline(image: &mut Image, rect: PixelRect, px: [u8; 4]) {
    let t = 3;
    let r = PixelRect {
        x: rect.x.saturating_sub(t),
        y: rect.y.saturating_sub(t),
        width: rect.width + 2 * t,
        height: rect.height + 2 * t,
    };
    let (w, h) = (image.width, image.height);
    for edge in [
        PixelRect { height: t, ..r },
        PixelRect {
            y: r.y + r.height.saturating_sub(t),
            height: t,
            ..r
        },
        PixelRect { width: t, ..r },
        PixelRect {
            x: r.x + r.width.saturating_sub(t),
            width: t,
            ..r
        },
    ] {
        image.fill(edge.clip(w, h), px);
    }
}

impl Region {
    /// Names the elements in this region: the innermost visible elements
    /// with an identifier or a label whose frames overlap it, at most three.
    pub fn locate(&mut self, tree: &UiNode) {
        let overlaps = |f: &Rect| {
            f.x < self.rect.x + self.rect.width
                && self.rect.x < f.x + f.width
                && f.y < self.rect.y + self.rect.height
                && self.rect.y < f.y + f.height
        };
        let mut found: Vec<&UiNode> = Vec::new();
        collect_innermost(tree, &overlaps, &mut found);
        self.elements = found.iter().take(3).map(|n| n.summary()).collect();
    }
}

/// Depth first: a node is kept only when none of its descendants is.
fn collect_innermost<'a>(
    node: &'a UiNode,
    overlaps: &dyn Fn(&Rect) -> bool,
    found: &mut Vec<&'a UiNode>,
) -> bool {
    if !node.visible && node.kind != "Application" {
        return false;
    }
    let Some(frame) = node.frame else {
        return false;
    };
    if !overlaps(&frame) {
        return false;
    }
    let mut any = false;
    for child in &node.children {
        any |= collect_innermost(child, overlaps, found);
    }
    let named = node.identifier.is_some() || node.label.is_some();
    if !any && named && node.kind != "Application" {
        found.push(node);
        return true;
    }
    any
}

/// One difference between two accessibility trees.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TreeChange {
    /// `removed`, `added`, `label`, `value`, `enabled` or `moved`.
    pub change: &'static str,
    /// The element, as `forge ui tree` prints it (the baseline's for
    /// `removed`, the new one otherwise).
    pub element: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

impl std::fmt::Display for TreeChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.before, &self.after) {
            (Some(b), Some(a)) => write!(f, "{} {}: {b} -> {a}", self.change, self.element),
            _ => write!(f, "{} {}", self.change, self.element),
        }
    }
}

/// Elements are matched by identifier, or by type and label when they have
/// no identifier; elements with neither are not compared. Frames that move
/// by less than 2 points are the same.
pub fn tree_changes(before: &UiNode, after: &UiNode) -> Vec<TreeChange> {
    fn keyed(tree: &UiNode) -> Vec<(String, &UiNode)> {
        let mut count: HashMap<String, usize> = HashMap::new();
        tree.walk()
            .filter(|n| n.visible && n.kind != "Application")
            .filter_map(|n| {
                let key = match (&n.identifier, &n.label) {
                    (Some(id), _) => format!("id={id}"),
                    (None, Some(label)) => format!("{} {label:?}", n.kind),
                    _ => return None,
                };
                let seen = count.entry(key.clone()).or_default();
                *seen += 1;
                let key = if *seen > 1 {
                    format!("{key}#{seen}")
                } else {
                    key
                };
                Some((key, n))
            })
            .collect()
    }
    let old = keyed(before);
    let new = keyed(after);
    let old_map: HashMap<&str, &UiNode> = old.iter().map(|(k, n)| (k.as_str(), *n)).collect();
    let new_keys: HashSet<&str> = new.iter().map(|(k, _)| k.as_str()).collect();
    let mut changes = Vec::new();
    for (key, node) in &old {
        if !new_keys.contains(key.as_str()) {
            changes.push(TreeChange {
                change: "removed",
                element: node.summary(),
                before: None,
                after: None,
            });
        }
    }
    let show = |v: &Option<String>| Some(v.clone().map_or("none".into(), |s| format!("{s:?}")));
    for (key, node) in &new {
        let Some(was) = old_map.get(key.as_str()) else {
            changes.push(TreeChange {
                change: "added",
                element: node.summary(),
                before: None,
                after: None,
            });
            continue;
        };
        let mut push = |change, before, after| {
            changes.push(TreeChange {
                change,
                element: node.summary(),
                before,
                after,
            })
        };
        if was.label != node.label {
            push("label", show(&was.label), show(&node.label));
        }
        if was.value != node.value {
            push("value", show(&was.value), show(&node.value));
        }
        if was.enabled != node.enabled {
            push(
                "enabled",
                Some(was.enabled.to_string()),
                Some(node.enabled.to_string()),
            );
        }
        if let (Some(a), Some(b)) = (was.frame, node.frame) {
            let far = |p: f64, q: f64| (p - q).abs() >= 2.0;
            if far(a.x, b.x) || far(a.y, b.y) || far(a.width, b.width) || far(a.height, b.height) {
                let r = |f: Rect| format!("{}x{} at ({}, {})", f.width, f.height, f.x, f.y);
                push("moved", Some(r(a)), Some(r(b)));
            }
        }
    }
    changes
}

/// Accessibility findings in `after` that `before` didn't have: what a
/// change made worse for VoiceOver users or for tests.
pub fn new_findings(before: &UiNode, after: &UiNode) -> Vec<String> {
    let old: HashSet<String> = before.audit().iter().map(|f| f.to_string()).collect();
    after
        .audit()
        .iter()
        .map(|f| f.to_string())
        .filter(|f| !old.contains(f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    fn rect(x: u32, y: u32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    fn opts() -> Options {
        Options {
            scale: 3.0,
            ..Options::default()
        }
    }

    #[test]
    fn png_round_trip_and_size() {
        let mut image = Image::new(7, 5, WHITE);
        image.fill(rect(1, 1, 2, 2), [10, 20, 30, 255]);
        let bytes = encode_png(&image).unwrap();
        assert_eq!(png_size(&bytes).unwrap(), (7, 5));
        assert_eq!(decode_png(&bytes).unwrap(), image);
        assert!(decode_png(b"not a png").is_err());
    }

    #[test]
    fn rgb_pngs_decode_to_rgba() {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[1, 2, 3, 4, 5, 6]).unwrap();
        }
        let image = decode_png(&out).unwrap();
        assert_eq!(image.rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn identical_images_match() {
        let a = Image::new(300, 600, WHITE);
        let c = compare(&a, &a.clone(), &opts());
        assert!(!c.changed());
        assert_eq!(c.changed_pixels, 0);
        assert_eq!(c.compared_pixels, 300 * 600);
    }

    #[test]
    fn small_colour_noise_is_within_tolerance() {
        let a = Image::new(100, 100, WHITE);
        let mut b = a.clone();
        b.fill(rect(10, 10, 50, 50), [245, 250, 240, 255]);
        assert!(!compare(&a, &b, &opts()).changed());
    }

    #[test]
    fn a_changed_word_counts_and_is_located() {
        let a = Image::new(300, 600, WHITE);
        let mut b = a.clone();
        // A 10 pt x 12 pt glyph block at (20 pt, 100 pt).
        b.fill(rect(60, 300, 30, 36), BLACK);
        let c = compare(&a, &b, &opts());
        assert!(c.changed());
        assert_eq!(c.regions.len(), 1);
        let region = &c.regions[0];
        assert_eq!(region.changed, 30 * 36);
        assert_eq!(region.rect.x, 20.0);
        assert_eq!(region.rect.y, 100.0);
        assert!(c.diff.is_some());
    }

    #[test]
    fn a_blinking_cursor_does_not_count_but_a_divider_line_does() {
        let a = Image::new(300, 600, WHITE);
        let mut cursor = a.clone();
        cursor.fill(rect(150, 300, 6, 60), BLACK); // 2 pt x 20 pt
        let c = compare(&a, &cursor, &opts());
        assert!(!c.changed());
        assert_eq!(c.ignored_regions, 1);
        assert!(c.changed_pixels > 0);

        let mut line = a.clone();
        line.fill(rect(0, 400, 300, 1), BLACK); // 100 pt x 1 px
        assert!(compare(&a, &line, &opts()).changed());
    }

    #[test]
    fn masked_areas_are_not_compared() {
        let a = Image::new(300, 600, WHITE);
        let mut b = a.clone();
        b.fill(rect(0, 0, 300, 50), BLACK);
        let mut o = opts();
        o.masks.push(status_bar(300, 600, 3.0));
        let c = compare(&a, &b, &o);
        assert!(!c.changed(), "{c:?}");
        assert_eq!(c.compared_pixels, 300 * (600 - 60));
    }

    #[test]
    fn different_sizes_are_a_change() {
        let c = compare(&Image::new(3, 3, WHITE), &Image::new(3, 4, WHITE), &opts());
        assert!(c.size_changed && c.changed() && c.diff.is_none());
    }

    #[test]
    fn status_bar_height_depends_on_the_screen() {
        // iPhone 15 Pro Max: 1290 x 2796 at 3x is 430 x 932 pt.
        assert_eq!(status_bar(1290, 2796, 3.0).height, 162);
        // iPhone SE: 750 x 1334 at 2x is 375 x 667 pt.
        assert_eq!(status_bar(750, 1334, 2.0).height, 40);
    }

    fn node(
        kind: &str,
        id: Option<&str>,
        label: Option<&str>,
        frame: (f64, f64, f64, f64),
    ) -> UiNode {
        UiNode {
            kind: kind.into(),
            identifier: id.map(Into::into),
            label: label.map(Into::into),
            value: None,
            enabled: true,
            visible: true,
            frame: Some(Rect {
                x: frame.0,
                y: frame.1,
                width: frame.2,
                height: frame.3,
            }),
            children: Vec::new(),
        }
    }

    fn screen(children: Vec<UiNode>) -> UiNode {
        let mut root = node("Application", None, Some("App"), (0.0, 0.0, 100.0, 200.0));
        root.visible = false;
        root.children = children;
        root
    }

    #[test]
    fn scale_comes_from_the_tree_or_a_guess() {
        let tree = screen(vec![]);
        assert_eq!(scale_of(300, Some(&tree)), 3.0);
        assert_eq!(scale_of(1290, None), 3.0);
        assert_eq!(scale_of(750, None), 2.0);
    }

    #[test]
    fn regions_name_the_innermost_elements_there() {
        let mut group = node("Other", None, None, (0.0, 0.0, 100.0, 100.0));
        group.children = vec![
            node(
                "Button",
                Some("save"),
                Some("Save"),
                (10.0, 10.0, 30.0, 20.0),
            ),
            node(
                "StaticText",
                None,
                Some("Far away"),
                (10.0, 80.0, 30.0, 10.0),
            ),
        ];
        let tree = screen(vec![group]);
        let mut region = Region {
            rect: Rect {
                x: 20.0,
                y: 15.0,
                width: 5.0,
                height: 5.0,
            },
            pixels: rect(0, 0, 1, 1),
            changed: 1,
            elements: Vec::new(),
        };
        region.locate(&tree);
        assert_eq!(region.elements, vec!["Button id=save \"Save\"".to_string()]);
    }

    #[test]
    fn tree_changes_report_what_happened_to_elements() {
        let mut save = node(
            "Button",
            Some("save"),
            Some("Save"),
            (10.0, 10.0, 30.0, 20.0),
        );
        let before = screen(vec![
            save.clone(),
            node(
                "Button",
                Some("delete"),
                Some("Delete"),
                (10.0, 40.0, 30.0, 20.0),
            ),
            node("StaticText", None, Some("Title"), (0.0, 0.0, 50.0, 10.0)),
        ]);
        save.label = Some("Done".into());
        save.enabled = false;
        save.frame.as_mut().unwrap().y = 30.0;
        let after = screen(vec![
            save,
            node("StaticText", None, Some("Title"), (0.0, 1.0, 50.0, 10.0)),
            node("Button", None, Some("Share"), (10.0, 70.0, 30.0, 20.0)),
        ]);
        let changes: Vec<String> = tree_changes(&before, &after)
            .iter()
            .map(|c| c.to_string())
            .collect();
        assert_eq!(
            changes,
            vec![
                "removed Button id=delete \"Delete\"",
                "label Button id=save \"Done\" (disabled): \"Save\" -> \"Done\"",
                "enabled Button id=save \"Done\" (disabled): true -> false",
                "moved Button id=save \"Done\" (disabled): 30x20 at (10, 10) -> 30x20 at (10, 30)",
                "added Button \"Share\"",
            ]
        );
        // The Share button has no identifier and is 30 x 20 pt: two new
        // accessibility findings.
        let worse = new_findings(&before, &after);
        assert_eq!(worse.len(), 2, "{worse:?}");
        assert!(worse[0].starts_with("no identifier"), "{worse:?}");
        assert!(
            worse[1].starts_with("tap target too small (30 x 20 pt"),
            "{worse:?}"
        );
        assert!(tree_changes(&before, &before).is_empty());
    }
}
