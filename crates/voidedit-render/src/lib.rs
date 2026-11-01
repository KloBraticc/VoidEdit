use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use ab_glyph::{Font, FontRef, ScaleFont};
use rayon::prelude::*;
use voidedit_core::{Blend, Clip, Effect, MaskShape, Project, Shape, TrackKind, Transition};

const FONT: &[u8] = include_bytes!("../../../assets/Inter-Medium.ttf");

#[derive(Default)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy)]
pub enum Op {
    Gain(f32),
    Mix([f32; 3], f32),
    Invert,
    Gray,
    Sepia,
    Contrast(f32),
    Saturation(f32),
    Warm(f32),
    Posterize(f32),
    Hue(f32),
    Gamma(f32),
    Levels(f32, f32),
    Balance([f32; 3]),
    Threshold(f32),
    Wheels([f32; 3], [f32; 3], [f32; 3]),
    Curve(f32, f32, f32),
    Solarize(f32),
    Vibrance(f32),
    Duotone([f32; 3], [f32; 3]),
    Swap,
}

pub struct Lut {
    size: usize,
    table: Vec<[f32; 3]>,
}

impl Lut {
    pub fn parse(text: &str) -> Option<Lut> {
        let mut size = 0;
        let mut table = Vec::new();
        for line in text.lines().map(str::trim) {
            if let Some(value) = line.strip_prefix("LUT_3D_SIZE") {
                size = value.trim().parse().ok()?;
            } else if line.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
                let mut parts = line.split_whitespace().map(str::parse::<f32>);
                table.push([parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?]);
            }
        }
        (size >= 2 && size <= 129 && table.len() == size * size * size).then_some(Lut { size, table })
    }

    fn apply(&self, color: [f32; 3]) -> [f32; 3] {
        let n = self.size - 1;
        let at = |channel: f32| ((channel / 255.0).clamp(0.0, 1.0) * n as f32).round() as usize;
        let value = self.table[at(color[2]) * self.size * self.size + at(color[1]) * self.size + at(color[0])];
        value.map(|channel| channel * 255.0)
    }
}

fn load_lut(path: &str) -> Option<Arc<Lut>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Lut>>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    cache.entry(path.to_string()).or_insert_with(|| std::fs::read_to_string(path).ok().and_then(|text| Lut::parse(&text)).map(Arc::new)).clone()
}

fn convolve(image: &Image, kernel: [f32; 9], keep_alpha: bool) -> Image {
    let (width, height) = (image.width, image.height);
    let mut pixels = image.pixels.clone();
    pixels.par_chunks_mut(width.max(1) * 4).enumerate().for_each(|(row, line)| {
        for column in 0..width {
            for channel in 0..3 {
                let mut sum = 0.0;
                for dy in 0..3usize {
                    for dx in 0..3usize {
                        let y = (row + dy).saturating_sub(1).min(height - 1);
                        let x = (column + dx).saturating_sub(1).min(width - 1);
                        sum += image.pixels[(y * width + x) * 4 + channel] as f32 * kernel[dy * 3 + dx];
                    }
                }
                line[column * 4 + channel] = sum.clamp(0.0, 255.0) as u8;
            }
            if !keep_alpha {
                line[column * 4 + 3] = 255;
            }
        }
    });
    Image { width, height, pixels }
}

fn glow(image: &Image, amount: f32) -> Image {
    let soft = blur(image, 6);
    let mut pixels = image.pixels.clone();
    for (pixel, halo) in pixels.chunks_exact_mut(4).zip(soft.pixels.chunks_exact(4)) {
        for channel in 0..3 {
            pixel[channel] = (pixel[channel] as f32 + halo[channel] as f32 * amount).min(255.0) as u8;
        }
    }
    Image { width: image.width, height: image.height, pixels }
}

pub struct Resolved {
    pub image: Option<Arc<Image>>,
    pub cos: f32,
    pub sin: f32,
    pub x: f32,
    pub y: f32,
    pub half_w: f32,
    pub half_h: f32,
    pub shape: Shape,
    pub color: [f32; 3],
    pub opacity: f32,
    pub ops: Vec<Op>,
    pub vignette: f32,
    pub pixelate: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub blend: Blend,
    pub noise: f32,
    pub key: Option<([f32; 3], f32, f32)>,
    pub mask: voidedit_core::Mask,
    pub wipe_x: f32,
    pub wipe_y: f32,
    pub lut: Option<Arc<Lut>>,
    pub wave: (f32, f32),
    pub space: Option<Space>,
    pub mirror: bool,
    pub warp: Warp,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Warp {
    pub aberration: f32,
    pub scanlines: (f32, f32),
    pub swirl: f32,
    pub segments: f32,
    pub fisheye: f32,
}

fn warp_uv(warp: &Warp, u: f32, v: f32) -> (f32, f32) {
    let (mut x, mut y) = (u - 0.5, v - 0.5);
    if warp.segments >= 2.0 {
        let radius = (x * x + y * y).sqrt();
        let wedge = std::f32::consts::TAU / warp.segments;
        let angle = (y.atan2(x).rem_euclid(wedge) - wedge / 2.0).abs();
        (x, y) = (radius * angle.cos(), radius * angle.sin());
    }
    if warp.swirl != 0.0 {
        let radius = (x * x + y * y).sqrt();
        let (sin, cos) = (warp.swirl * (1.0 - (radius * 2.0).min(1.0))).sin_cos();
        (x, y) = (x * cos - y * sin, x * sin + y * cos);
    }
    if warp.fisheye != 0.0 {
        let radius = (x * x + y * y).sqrt() * 2.0;
        let bend = 1.0 + warp.fisheye * radius * radius;
        (x, y) = (x * bend, y * bend);
    }
    ((x + 0.5).clamp(0.0, 1.0), (y + 0.5).clamp(0.0, 1.0))
}

fn smooth_points(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut current = points.to_vec();
    for _ in 0..3 {
        let mut next = Vec::with_capacity(current.len() * 2);
        for index in 0..current.len() {
            let (a, b) = (current[index], current[(index + 1) % current.len()]);
            next.push([a[0] * 0.75 + b[0] * 0.25, a[1] * 0.75 + b[1] * 0.25]);
            next.push([a[0] * 0.25 + b[0] * 0.75, a[1] * 0.25 + b[1] * 0.75]);
        }
        current = next;
    }
    current
}

#[derive(Clone, Copy)]
pub struct Space {
    pub across: [f32; 3],
    pub down: [f32; 3],
    pub normal: [f32; 3],
    pub depth: f32,
    pub camera: f32,
}

const CAMERA_DISTANCE: f32 = 2000.0;

fn space_of(clip: &Clip, frame: f32, scale: f32) -> Option<Space> {
    let (rx, ry, depth) = (clip.rotation_x.at(frame), clip.rotation_y.at(frame), clip.depth.at(frame));
    if rx == 0.0 && ry == 0.0 && depth == 0.0 {
        return None;
    }
    let (sx, cx) = rx.to_radians().sin_cos();
    let (sy, cy) = ry.to_radians().sin_cos();
    Some(Space { across: [cy, 0.0, -sy], down: [sy * sx, cx, cy * sx], normal: [sy * cx, -sx, cy * cx], depth: depth * scale, camera: CAMERA_DISTANCE * scale })
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn project_local(space: &Space, dx: f32, dy: f32) -> Option<(f32, f32)> {
    let direction = [dx, dy, space.camera];
    let offset = [0.0, 0.0, space.depth + space.camera];
    let denominator = dot3(space.normal, direction);
    if denominator.abs() < 1e-5 {
        return None;
    }
    let t = dot3(space.normal, offset) / denominator;
    if t <= 0.0 {
        return None;
    }
    let hit = [direction[0] * t, direction[1] * t, direction[2] * t - offset[2]];
    Some((dot3(space.across, hit), dot3(space.down, hit)))
}

fn blend(mode: Blend, base: f32, top: f32) -> f32 {
    let (b, t) = (base / 255.0, top / 255.0);
    let result = match mode {
        Blend::Normal => t,
        Blend::Multiply => b * t,
        Blend::Screen => 1.0 - (1.0 - b) * (1.0 - t),
        Blend::Overlay => {
            if b < 0.5 {
                2.0 * b * t
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - t)
            }
        }
        Blend::Add => b + t,
        Blend::Darken => b.min(t),
        Blend::Lighten => b.max(t),
        Blend::Difference => (b - t).abs(),
        Blend::SoftLight => (1.0 - 2.0 * t) * b * b + 2.0 * t * b,
    };
    result * 255.0
}

fn mask_alpha(mask: &voidedit_core::Mask, u: f32, v: f32) -> f32 {
    if mask.shape == MaskShape::None {
        return 1.0;
    }
    if mask.shape == MaskShape::Polygon {
        if mask.points.len() < 3 {
            return 1.0;
        }
        let mut inside = false;
        let smoothed;
        let points: &[[f32; 2]] = if mask.smooth {
            smoothed = smooth_points(&mask.points);
            &smoothed
        } else {
            &mask.points
        };
        let mut previous = points[points.len() - 1];
        for point in points {
            if (point[1] > v) != (previous[1] > v) && u < (previous[0] - point[0]) * (v - point[1]) / (previous[1] - point[1]) + point[0] {
                inside = !inside;
            }
            previous = *point;
        }
        return if inside != mask.invert { 1.0 } else { 0.0 };
    }
    let (dx, dy) = ((u - mask.x) / (mask.width / 2.0).max(0.001), (v - mask.y) / (mask.height / 2.0).max(0.001));
    let distance = match mask.shape {
        MaskShape::Ellipse => (dx * dx + dy * dy).sqrt(),
        _ => dx.abs().max(dy.abs()),
    };
    let feather = mask.feather.max(0.001) / (mask.width.min(mask.height) / 2.0).max(0.001);
    let alpha = ((1.0 - distance) / feather + 0.5).clamp(0.0, 1.0);
    if mask.invert {
        1.0 - alpha
    } else {
        alpha
    }
}

fn apply(ops: &[Op], mut color: [f32; 3]) -> [f32; 3] {
    for op in ops {
        match *op {
            Op::Gain(gain) => color = color.map(|channel| channel * gain),
            Op::Mix(tint, mix) => {
                for index in 0..3 {
                    color[index] += (tint[index] - color[index]) * mix;
                }
            }
            Op::Invert => color = color.map(|channel| 255.0 - channel),
            Op::Gray => color = [luma(color); 3],
            Op::Sepia => {
                let [r, g, b] = color;
                color = [r * 0.393 + g * 0.769 + b * 0.189, r * 0.349 + g * 0.686 + b * 0.168, r * 0.272 + g * 0.534 + b * 0.131];
            }
            Op::Contrast(amount) => color = color.map(|channel| (channel - 127.5) * amount + 127.5),
            Op::Saturation(amount) => {
                let gray = luma(color);
                color = color.map(|channel| gray + (channel - gray) * amount);
            }
            Op::Warm(amount) => {
                color[0] += amount;
                color[2] -= amount;
            }
            Op::Posterize(levels) => color = color.map(|channel| (channel / 255.0 * levels).round() / levels * 255.0),
            Op::Hue(radians) => {
                let (sin, cos) = radians.sin_cos();
                let [r, g, b] = color;
                color = [
                    r * (0.213 + cos * 0.787 - sin * 0.213) + g * (0.715 - cos * 0.715 - sin * 0.715) + b * (0.072 - cos * 0.072 + sin * 0.928),
                    r * (0.213 - cos * 0.213 + sin * 0.143) + g * (0.715 + cos * 0.285 + sin * 0.14) + b * (0.072 - cos * 0.072 - sin * 0.283),
                    r * (0.213 - cos * 0.213 - sin * 0.787) + g * (0.715 - cos * 0.715 + sin * 0.715) + b * (0.072 + cos * 0.928 + sin * 0.072),
                ];
            }
            Op::Gamma(power) => color = color.map(|channel| (channel.max(0.0) / 255.0).powf(power) * 255.0),
            Op::Levels(black, white) => color = color.map(|channel| (channel - black) / (white - black).max(1.0) * 255.0),
            Op::Balance(shift) => {
                for index in 0..3 {
                    color[index] += shift[index];
                }
            }
            Op::Threshold(level) => color = [if luma(color) >= level { 255.0 } else { 0.0 }; 3],
            Op::Wheels(lift, gamma, gain) => {
                for index in 0..3 {
                    let value = (color[index] / 255.0 * gain[index] + lift[index]).max(0.0);
                    color[index] = value.powf(1.0 / gamma[index].max(0.05)) * 255.0;
                }
            }
            Op::Swap => color = [color[2], color[0], color[1]],
            Op::Solarize(level) => color = color.map(|channel| if channel > level { 255.0 - channel } else { channel }),
            Op::Vibrance(amount) => {
                let gray = luma(color);
                let peak = color[0].max(color[1]).max(color[2]);
                let low = color[0].min(color[1]).min(color[2]);
                let boost = 1.0 + amount * (1.0 - (peak - low) / 255.0);
                color = color.map(|channel| gray + (channel - gray) * boost);
            }
            Op::Duotone(dark, light) => {
                let t = (luma(color) / 255.0).clamp(0.0, 1.0);
                color = [dark[0] + (light[0] - dark[0]) * t, dark[1] + (light[1] - dark[1]) * t, dark[2] + (light[2] - dark[2]) * t];
            }
            Op::Curve(shadows, midtones, highlights) => {
                color = color.map(|channel| {
                    let t = (channel / 255.0).clamp(0.0, 1.0);
                    let low = (1.0 - t) * (1.0 - t);
                    let mid = 2.0 * t * (1.0 - t);
                    let high = t * t;
                    channel + (low * shadows + mid * midtones + high * highlights) * 1.27
                });
            }
        }
    }
    color
}

fn luma(color: [f32; 3]) -> f32 {
    color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722
}

fn blur(image: &Image, radius: usize) -> Image {
    let (width, height) = (image.width, image.height);
    let mut pixels = image.pixels.clone();
    if radius == 0 || width == 0 || height == 0 {
        return Image { width, height, pixels };
    }
    for _ in 0..2 {
        let source = pixels.clone();
        pixels.par_chunks_mut(width * 4).enumerate().for_each(|(row, line)| {
            let input = &source[row * width * 4..(row + 1) * width * 4];
            box_line(input, line, width, 4, radius);
        });
        let source = pixels.clone();
        let mut rotated = vec![0u8; pixels.len()];
        rotated.par_chunks_mut(height * 4).enumerate().for_each(|(column, line)| {
            for row in 0..height {
                line[row * 4..row * 4 + 4].copy_from_slice(&source[(row * width + column) * 4..(row * width + column) * 4 + 4]);
            }
        });
        let mut done = vec![0u8; pixels.len()];
        done.par_chunks_mut(height * 4).zip(rotated.par_chunks(height * 4)).for_each(|(out, input)| box_line(input, out, height, 4, radius));
        for column in 0..width {
            for row in 0..height {
                pixels[(row * width + column) * 4..(row * width + column) * 4 + 4].copy_from_slice(&done[(column * height + row) * 4..(column * height + row) * 4 + 4]);
            }
        }
    }
    Image { width, height, pixels }
}

fn box_line(input: &[u8], output: &mut [u8], count: usize, channels: usize, radius: usize) {
    let window = (radius * 2 + 1) as u32;
    for channel in 0..channels {
        let at = |index: i64| input[index.clamp(0, count as i64 - 1) as usize * channels + channel] as u32;
        let mut sum: u32 = (-(radius as i64)..=radius as i64).map(at).sum();
        for index in 0..count {
            output[index * channels + channel] = (sum / window) as u8;
            sum += at(index as i64 + radius as i64 + 1);
            sum -= at(index as i64 - radius as i64);
        }
    }
}

fn font() -> Option<&'static FontRef<'static>> {
    static LOADED: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
    LOADED.get_or_init(|| FontRef::try_from_slice(FONT).ok()).as_ref()
}

pub fn text_image(text: &str, size: f32, color: [u8; 3], stroke: f32, background: bool) -> Option<Image> {
    let font = font()?;
    let size = size.clamp(4.0, 800.0);
    let scaled = font.as_scaled(size);
    let lines: Vec<&str> = text.split('\n').collect();
    let line_height = scaled.height() + scaled.line_gap();
    let measure = |line: &str| line.chars().map(|c| scaled.h_advance(font.glyph_id(c))).sum::<f32>();
    let width = (lines.iter().map(|line| measure(line)).fold(1.0f32, f32::max).ceil() as usize + 2).min(8192);
    let height = ((line_height * lines.len() as f32).ceil() as usize + 2).min(8192);
    let mut pixels = vec![0u8; width * height * 4];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[..3].copy_from_slice(&color);
    }
    for (row, line) in lines.iter().enumerate() {
        let mut pen = (width as f32 - measure(line)) / 2.0;
        let baseline = scaled.ascent() + row as f32 * line_height;
        for c in line.chars() {
            let id = font.glyph_id(c);
            let glyph = id.with_scale_and_position(size, ab_glyph::point(pen, baseline));
            pen += scaled.h_advance(id);
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    let px = gx as i64 + bounds.min.x as i64;
                    let py = gy as i64 + bounds.min.y as i64;
                    if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                        let alpha = &mut pixels[(py as usize * width + px as usize) * 4 + 3];
                        *alpha = (*alpha).max((coverage * 255.0) as u8);
                    }
                });
            }
        }
    }
    if stroke > 0.0 {
        let radius = stroke.min(32.0) as i64;
        let source = pixels.clone();
        for py in 0..height as i64 {
            for px in 0..width as i64 {
                let start = (py as usize * width + px as usize) * 4;
                if source[start + 3] > 128 {
                    continue;
                }
                let near = (-radius..=radius).any(|dy| {
                    (-radius..=radius).any(|dx| {
                        let (nx, ny) = (px + dx, py + dy);
                        nx >= 0 && ny >= 0 && (nx as usize) < width && (ny as usize) < height && source[(ny as usize * width + nx as usize) * 4 + 3] > 128
                    })
                });
                if near {
                    pixels[start..start + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
    }
    if background {
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = pixel[3] as f32 / 255.0;
            for channel in &mut pixel[..3] {
                *channel = (*channel as f32 * alpha) as u8;
            }
            pixel[3] = pixel[3].max(170);
        }
    }
    Some(Image { width, height, pixels })
}

fn resolve(clip: &Clip, frame: f32, scale: f32, image: Option<Arc<Image>>) -> Option<Resolved> {
    let opacity = clip.opacity.at(frame).clamp(0.0, 100.0) / 100.0 * clip.fade(frame);
    let size = clip.scale.at(frame).max(0.0) / 100.0 * scale;
    if opacity <= 0.0 || size <= 0.0 {
        return None;
    }
    let mut ops = Vec::new();
    let mut vignette = 0.0;
    let mut pixelate = 0.0;
    let mut radius = 0.0f32;
    let (mut flip_x, mut flip_y) = (false, false);
    let mut mirror = false;
    for effect in &clip.effects {
        match effect {
            Effect::Brightness { amount } => ops.push(Op::Gain(1.0 + amount.at(frame) / 100.0)),
            Effect::Exposure { amount } => ops.push(Op::Gain(2f32.powf(amount.at(frame) / 25.0))),
            Effect::Tint { amount, color } => ops.push(Op::Mix(color.map(|channel| channel as f32), amount.at(frame).clamp(0.0, 100.0) / 100.0)),
            Effect::Invert => ops.push(Op::Invert),
            Effect::Grayscale => ops.push(Op::Gray),
            Effect::Sepia => ops.push(Op::Sepia),
            Effect::Contrast { amount } => ops.push(Op::Contrast((1.0 + amount.at(frame) / 100.0).max(0.0))),
            Effect::Saturation { amount } => ops.push(Op::Saturation((1.0 + amount.at(frame) / 100.0).max(0.0))),
            Effect::Temperature { amount } => ops.push(Op::Warm(amount.at(frame) * 0.5)),
            Effect::Posterize { levels } => ops.push(Op::Posterize(levels.at(frame).max(2.0))),
            Effect::Vignette { amount } => vignette = amount.at(frame).clamp(0.0, 100.0) / 100.0,
            Effect::Pixelate { size } => pixelate = size.at(frame).max(0.0),
            Effect::Blur { radius: value } => radius = value.at(frame).max(0.0),
            Effect::FlipHorizontal => flip_x = !flip_x,
            Effect::FlipVertical => flip_y = !flip_y,
            Effect::ColorWheels { lift, gamma, gain } => ops.push(Op::Wheels(*lift, *gamma, *gain)),
            Effect::Curves { shadows, midtones, highlights } => ops.push(Op::Curve(shadows.at(frame), midtones.at(frame), highlights.at(frame))),
            Effect::Lut { .. } | Effect::Sharpen { .. } | Effect::Glow { .. } | Effect::EdgeDetect | Effect::DropShadow { .. } | Effect::Wave { .. } => {}
            Effect::Solarize { level } => ops.push(Op::Solarize(level.at(frame))),
            Effect::Vibrance { amount } => ops.push(Op::Vibrance(amount.at(frame) / 100.0)),
            Effect::Duotone { dark, light } => ops.push(Op::Duotone(dark.map(|channel| channel as f32), light.map(|channel| channel as f32))),
            Effect::Mirror => mirror = true,
            Effect::ChannelSwap => ops.push(Op::Swap),
            Effect::Emboss | Effect::ChromaticAberration { .. } | Effect::Scanlines { .. } | Effect::Swirl { .. } | Effect::Kaleidoscope { .. } | Effect::Fisheye { .. } => {}
            Effect::HueShift { degrees } => ops.push(Op::Hue(degrees.at(frame).to_radians())),
            Effect::Gamma { amount } => ops.push(Op::Gamma(1.0 / amount.at(frame).max(0.05))),
            Effect::Levels { black, white } => ops.push(Op::Levels(black.at(frame), white.at(frame))),
            Effect::ColorBalance { red, green, blue } => ops.push(Op::Balance([red.at(frame), green.at(frame), blue.at(frame)])),
            Effect::Threshold { level } => ops.push(Op::Threshold(level.at(frame))),
            Effect::Noise { .. } | Effect::ChromaKey { .. } => {}
        }
    }
    let mut image = match (image, radius as usize) {
        (Some(image), 0) => Some(image),
        (Some(image), radius) => Some(Arc::new(blur(&image, radius.min(64)))),
        (None, _) => None,
    };
    let mut lut = None;
    let mut wave = (0.0, 0.0);
    let mut warp = Warp::default();
    for effect in &clip.effects {
        match effect {
            Effect::Lut { path } if !path.is_empty() => lut = load_lut(path),
            Effect::Wave { amplitude, frequency } => wave = (amplitude.at(frame) * scale, frequency.at(frame)),
            Effect::Sharpen { amount } => {
                if let Some(current) = &image {
                    let a = amount.at(frame).clamp(0.0, 200.0) / 100.0;
                    image = Some(Arc::new(convolve(current, [0.0, -a, 0.0, -a, 1.0 + 4.0 * a, -a, 0.0, -a, 0.0], true)));
                }
            }
            Effect::Glow { amount } => {
                if let Some(current) = &image {
                    image = Some(Arc::new(glow(current, amount.at(frame).clamp(0.0, 200.0) / 100.0)));
                }
            }
            Effect::Emboss => {
                if let Some(current) = &image {
                    image = Some(Arc::new(convolve(current, [-2.0, -1.0, 0.0, -1.0, 1.0, 1.0, 0.0, 1.0, 2.0], true)));
                }
            }
            Effect::ChromaticAberration { amount } => warp.aberration = amount.at(frame) * scale,
            Effect::Scanlines { strength, spacing } => warp.scanlines = (strength.at(frame).clamp(0.0, 100.0) / 100.0, (spacing.at(frame) * scale).max(1.0)),
            Effect::Swirl { angle } => warp.swirl = angle.at(frame).to_radians(),
            Effect::Kaleidoscope { segments } => warp.segments = segments.at(frame).clamp(0.0, 32.0),
            Effect::Fisheye { amount } => warp.fisheye = amount.at(frame) / 100.0,
            Effect::EdgeDetect => {
                if let Some(current) = &image {
                    image = Some(Arc::new(convolve(current, [-1.0, -1.0, -1.0, -1.0, 8.0, -1.0, -1.0, -1.0, -1.0], false)));
                }
            }
            _ => {}
        }
    }
    let mut noise = 0.0;
    let mut key = None;
    for effect in &clip.effects {
        match effect {
            Effect::Noise { amount } => noise = amount.at(frame).max(0.0),
            Effect::ChromaKey { color, tolerance, softness } => key = Some((color.map(|channel| channel as f32), tolerance.at(frame).max(0.0) * 2.55, softness.at(frame).max(0.1) * 2.55)),
            _ => {}
        }
    }
    let (entering, leaving) = clip.progress(frame);
    let (mut x, mut y, mut zoom) = (clip.x.at(frame) * scale, clip.y.at(frame) * scale, 1.0f32);
    let (mut wipe_x, mut wipe_y) = (1.0f32, 1.0f32);
    for (kind, amount) in [(clip.transition_in, entering), (clip.transition_out, leaving)] {
        let eased = amount * amount * (3.0 - 2.0 * amount);
        match kind {
            Transition::Fade => {}
            Transition::WipeRight => wipe_x = wipe_x.min(amount),
            Transition::WipeDown => wipe_y = wipe_y.min(amount),
            Transition::SlideLeft => x += (1.0 - eased) * clip.width.max(1.0) * size,
            Transition::SlideUp => y += (1.0 - eased) * clip.height.max(1.0) * size,
            Transition::Zoom => zoom *= eased.max(0.001),
        }
    }
    let size = size * zoom;
    let (width, height) = match (&clip.text, &image) {
        (Some(_), Some(image)) => (image.width as f32, image.height as f32),
        _ => (clip.width, clip.height),
    };
    let (sin, cos) = (-clip.rotation.at(frame).to_radians()).sin_cos();
    Some(Resolved {
        color: apply(&ops, clip.color.map(|channel| channel as f32)),
        image,
        cos,
        sin,
        x,
        y,
        half_w: width * size / 2.0,
        half_h: height * size / 2.0,
        shape: clip.shape,
        opacity,
        ops,
        vignette,
        pixelate,
        flip_x,
        flip_y,
        blend: clip.blend,
        noise,
        key,
        mask: clip.mask.clone(),
        wipe_x,
        wipe_y,
        lut,
        wave,
        space: space_of(clip, frame, scale),
        mirror,
        warp,
    })
}

fn sample(layer: &Resolved, lx: f32, ly: f32) -> Option<([f32; 3], f32)> {
    let image = layer.image.as_ref()?;
    let mut u = (lx + layer.half_w) / (layer.half_w * 2.0);
    let mut v = (ly + layer.half_h) / (layer.half_h * 2.0);
    if layer.flip_x {
        u = 1.0 - u;
    }
    if layer.flip_y {
        v = 1.0 - v;
    }
    if layer.mirror {
        u = 1.0 - (u * 2.0 - 1.0).abs();
    }
    (u, v) = warp_uv(&layer.warp, u, v);
    if layer.pixelate > 1.0 {
        let (step_u, step_v) = (layer.pixelate / image.width.max(1) as f32, layer.pixelate / image.height.max(1) as f32);
        u = (u / step_u).floor() * step_u;
        v = (v / step_v).floor() * step_v;
    }
    let column = ((u * image.width as f32) as usize).min(image.width.saturating_sub(1));
    let row = ((v * image.height as f32) as usize).min(image.height.saturating_sub(1));
    let start = (row * image.width + column) * 4;
    let pixel = image.pixels.get(start..start + 4)?;
    let mut source = [pixel[0] as f32, pixel[1] as f32, pixel[2] as f32];
    if layer.warp.aberration != 0.0 {
        let shift = (layer.warp.aberration * image.width as f32 / (layer.half_w * 2.0).max(1.0)) as i64;
        let fetch = |offset: i64, channel: usize| image.pixels.get((row * image.width + (column as i64 + offset).clamp(0, image.width as i64 - 1) as usize) * 4 + channel).copied().unwrap_or(0) as f32;
        source[0] = fetch(shift, 0);
        source[2] = fetch(-shift, 2);
    }
    let mut alpha = pixel[3] as f32 / 255.0;
    if let Some((key, tolerance, softness)) = layer.key {
        let distance = (0..3).map(|index| (source[index] - key[index]).powi(2)).sum::<f32>().sqrt();
        alpha *= ((distance - tolerance) / softness).clamp(0.0, 1.0);
    }
    Some((apply(&layer.ops, source), alpha))
}

fn draw(layer: &Resolved, px: f32, py: f32) -> Option<([f32; 3], f32)> {
    let (mut dx, mut dy) = (px - layer.x, py - layer.y);
    if let Some(space) = &layer.space {
        (dx, dy) = project_local(space, dx, dy)?;
    }
    let (mut lx, ly) = (dx * layer.cos - dy * layer.sin, dx * layer.sin + dy * layer.cos);
    if layer.wave.0 != 0.0 && layer.half_h > 0.0 {
        lx += layer.wave.0 * (ly / layer.half_h * layer.wave.1).sin();
    }
    if layer.half_w <= 0.0 || layer.half_h <= 0.0 {
        return None;
    }
    let (color, mut alpha) = if layer.image.is_some() {
        if lx.abs() >= layer.half_w || ly.abs() >= layer.half_h {
            return None;
        }
        sample(layer, lx, ly)?
    } else {
        let coverage = match layer.shape {
            Shape::Solid | Shape::Rectangle => ((layer.half_w - lx.abs()).min(layer.half_h - ly.abs()) + 0.5).clamp(0.0, 1.0),
            Shape::Ellipse => {
                let (nx, ny) = (lx / layer.half_w, ly / layer.half_h);
                ((1.0 - (nx * nx + ny * ny).sqrt()) * layer.half_w.min(layer.half_h) + 0.5).clamp(0.0, 1.0)
            }
        };
        (layer.color, coverage)
    };
    alpha *= layer.opacity;
    let (u, v) = ((lx + layer.half_w) / (layer.half_w * 2.0), (ly + layer.half_h) / (layer.half_h * 2.0));
    if u > layer.wipe_x || v > layer.wipe_y {
        return None;
    }
    alpha *= mask_alpha(&layer.mask, u, v);
    if alpha <= 0.0 {
        return None;
    }
    let mut color = color;
    if layer.vignette > 0.0 {
        let (nx, ny) = (lx / layer.half_w, ly / layer.half_h);
        let falloff = 1.0 - layer.vignette * ((nx * nx + ny * ny) / 2.0).min(1.0);
        color = color.map(|channel| channel * falloff);
    }
    if layer.warp.scanlines.0 > 0.0 && (py / layer.warp.scanlines.1) as i64 % 2 == 0 {
        color = color.map(|channel| channel * (1.0 - layer.warp.scanlines.0));
    }
    if let Some(lut) = &layer.lut {
        color = lut.apply(color);
    }
    if layer.noise > 0.0 {
        let seed = (px as u32).wrapping_mul(73_856_093) ^ (py as u32).wrapping_mul(19_349_663) ^ (layer.x as u32).wrapping_mul(83_492_791);
        let grain = (seed.wrapping_mul(seed | 1) >> 24) as f32 / 255.0 - 0.5;
        color = color.map(|channel| channel + grain * layer.noise * 2.55);
    }
    Some((color, alpha))
}

pub fn wanted_frame(project: &Project, clip: &Clip, frame: u32) -> bool {
    clip.visible && frame >= clip.start && frame < clip.end && project.tracks.get(clip.track).is_some_and(|track| track.kind == TrackKind::Video)
}

pub fn render(project: &Project, frame: u32, scale: f32, target: &mut Frame, images: &dyn Fn(&str) -> Option<Arc<Image>>) {
    let scale = scale.clamp(0.05, 1.0);
    let width = ((project.width as f32 * scale) as usize).max(1);
    let height = ((project.height as f32 * scale) as usize).max(1);
    target.width = width;
    target.height = height;
    target.pixels.resize(width * height * 4, 255);
    let layers = layers(project, frame, scale, images);
    let background = project.background.map(|channel| channel as f32);
    target.pixels.par_chunks_mut(width * 4).enumerate().for_each(|(row, line)| {
        let py = row as f32 + 0.5;
        for (column, pixel) in line.chunks_exact_mut(4).enumerate() {
            let px = column as f32 + 0.5;
            let mut color = background;
            for layer in &layers {
                if let Some((source, alpha)) = draw(layer, px, py) {
                    for index in 0..3 {
                        let mixed = if layer.blend == Blend::Normal { source[index] } else { blend(layer.blend, color[index], source[index]) };
                        color[index] += (mixed - color[index]) * alpha;
                    }
                }
            }
            pixel[0] = color[0].clamp(0.0, 255.0) as u8;
            pixel[1] = color[1].clamp(0.0, 255.0) as u8;
            pixel[2] = color[2].clamp(0.0, 255.0) as u8;
            pixel[3] = 255;
        }
    });
}

pub fn layers(project: &Project, frame: u32, scale: f32, images: &dyn Fn(&str) -> Option<Arc<Image>>) -> Vec<Resolved> {
    let scale = scale.clamp(0.05, 1.0);
    let time = frame as f32;
    let mut order: Vec<&Clip> = project.clips.iter().filter(|clip| wanted_frame(project, clip, frame)).collect();
    order.sort_by_key(|clip| clip.track);
    order
        .into_iter()
        .filter_map(|clip| {
            if let Some(inner) = &clip.nested {
                let mut nested = Frame::default();
                render(inner, frame - clip.start + clip.offset, scale, &mut nested, images);
                let image = Arc::new(Image { width: nested.width, height: nested.height, pixels: nested.pixels });
                return Some((clip, resolve(clip, time, scale, Some(image))?));
            }
            let image = match (&clip.text, &clip.media) {
                (Some(text), _) => Some(Arc::new(text_image(text, clip.text_size, clip.color, clip.text_stroke, clip.text_background)?)),
                (None, Some(path)) => Some(images(path)?),
                (None, None) => None,
            };
            let main = resolve(clip, time, scale, image)?;
            Some((clip, main))
        })
        .flat_map(|(clip, main)| {
            let shadow = clip.effects.iter().find_map(|effect| match effect {
                Effect::DropShadow { x, y, opacity } => Some(Resolved {
                    image: main.image.clone(),
                    cos: main.cos,
                    sin: main.sin,
                    x: main.x + x.at(time) * scale,
                    y: main.y + y.at(time) * scale,
                    half_w: main.half_w,
                    half_h: main.half_h,
                    shape: main.shape,
                    color: [0.0; 3],
                    opacity: main.opacity * opacity.at(time).clamp(0.0, 100.0) / 100.0,
                    ops: vec![Op::Gain(0.0)],
                    vignette: 0.0,
                    pixelate: 0.0,
                    flip_x: main.flip_x,
                    flip_y: main.flip_y,
                    blend: Blend::Normal,
                    noise: 0.0,
                    key: main.key,
                    mask: main.mask.clone(),
                    wipe_x: main.wipe_x,
                    wipe_y: main.wipe_y,
                    lut: None,
                    wave: main.wave,
                    space: main.space,
                    mirror: main.mirror,
                    warp: main.warp,
                }),
                _ => None,
            });
            shadow.into_iter().chain(std::iter::once(main))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Project {
        Project { width: 16, height: 16, ..Project::default() }
    }

    fn first(project: &Project) -> [u8; 3] {
        let mut frame = Frame::default();
        render(project, 0, 1.0, &mut frame, &|_| None);
        [frame.pixels[0], frame.pixels[1], frame.pixels[2]]
    }

    #[test]
    fn higher_track_wins() {
        let mut project = small();
        let mut bottom = project.new_clip(Shape::Solid, 0);
        bottom.track = 1;
        bottom.color = [10, 20, 30];
        let mut top = project.new_clip(Shape::Solid, 0);
        top.track = 2;
        top.color = [200, 100, 50];
        project.clips = vec![top, bottom];
        assert_eq!(first(&project), [200, 100, 50]);
    }

    #[test]
    fn out_of_range_clips_do_not_draw() {
        let mut project = small();
        let clip = project.new_clip(Shape::Solid, 5);
        project.clips.push(clip);
        assert_eq!(first(&project), [0, 0, 0]);
    }

    #[test]
    fn every_effect_runs_without_crashing() {
        for name in voidedit_core::EFFECT_NAMES {
            let mut project = small();
            let mut clip = project.new_media_clip("a", 0, Some((8, 8)), 1.0);
            clip.effects.push(Effect::create(name).unwrap());
            project.clips.push(clip);
            let image = Arc::new(Image { width: 8, height: 8, pixels: vec![120; 256] });
            render(&project, 0, 1.0, &mut Frame::default(), &|_| Some(image.clone()));
        }
    }

    #[test]
    fn cube_luts_parse_and_apply() {
        let text = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        let lut = Lut::parse(text).unwrap();
        assert_eq!(lut.apply([255.0, 0.0, 0.0]), [255.0, 0.0, 0.0]);
        assert!(Lut::parse("LUT_3D_SIZE 2\n0 0 0").is_none());
    }

    #[test]
    fn flat_space_matches_no_space() {
        let space = Space { across: [1.0, 0.0, 0.0], down: [0.0, 1.0, 0.0], normal: [0.0, 0.0, 1.0], depth: 0.0, camera: 2000.0 };
        let (x, y) = project_local(&space, 30.0, -20.0).unwrap();
        assert!((x - 30.0).abs() < 0.01 && (y + 20.0).abs() < 0.01);
    }

    #[test]
    fn nested_clips_render() {
        let mut project = small();
        let mut clip = project.new_clip(Shape::Solid, 0);
        clip.color = [50, 60, 70];
        project.clips.push(clip);
        project.nest(0);
        assert_eq!(first(&project), [50, 60, 70]);
    }

    #[test]
    fn multiply_blend_darkens() {
        let mut project = small();
        let mut bottom = project.new_clip(Shape::Solid, 0);
        bottom.track = 1;
        bottom.color = [200, 200, 200];
        let mut top = project.new_clip(Shape::Solid, 0);
        top.track = 2;
        top.color = [100, 100, 100];
        top.blend = Blend::Multiply;
        project.clips = vec![bottom, top];
        assert!(first(&project)[0] < 100);
    }

    #[test]
    fn mask_hides_outside() {
        let mut project = small();
        let mut clip = project.new_clip(Shape::Solid, 0);
        clip.color = [255, 255, 255];
        clip.mask.shape = MaskShape::Ellipse;
        clip.mask.width = 0.2;
        clip.mask.height = 0.2;
        project.clips.push(clip);
        assert_eq!(first(&project), [0, 0, 0]);
    }

    #[test]
    fn chroma_key_removes_green() {
        let mut project = small();
        let mut clip = project.new_media_clip("a", 0, Some((16, 16)), 1.0);
        clip.effects.push(Effect::create("Chroma Key").unwrap());
        project.clips.push(clip);
        let image = Arc::new(Image { width: 1, height: 1, pixels: vec![0, 255, 0, 255] });
        let mut frame = Frame::default();
        render(&project, 0, 1.0, &mut frame, &|_| Some(image.clone()));
        assert_eq!(&frame.pixels[..3], &[0, 0, 0]);
    }

    #[test]
    fn invert_changes_shapes() {
        let mut project = small();
        let mut clip = project.new_clip(Shape::Solid, 0);
        clip.color = [255, 255, 255];
        clip.effects.push(Effect::Invert);
        project.clips.push(clip);
        assert_eq!(first(&project), [0, 0, 0]);
    }

    #[test]
    fn text_draws_pixels() {
        let image = text_image("Hi", 40.0, [255, 255, 255], 2.0, true).unwrap();
        assert!(image.pixels.chunks_exact(4).any(|pixel| pixel[3] > 200));
    }

    #[test]
    fn zero_size_shapes_do_not_crash() {
        let mut project = small();
        let mut clip = project.new_clip(Shape::Ellipse, 0);
        clip.width = 0.0;
        project.clips.push(clip);
        first(&project);
    }
}
