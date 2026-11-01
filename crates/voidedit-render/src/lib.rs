use std::sync::{Arc, OnceLock};

use ab_glyph::{Font, FontRef, ScaleFont};
use rayon::prelude::*;
use voidedit_core::{Clip, Effect, Project, Shape, TrackKind};

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
enum Op {
    Gain(f32),
    Mix([f32; 3], f32),
    Invert,
    Gray,
    Sepia,
    Contrast(f32),
    Saturation(f32),
    Warm(f32),
    Posterize(f32),
}

struct Resolved {
    image: Option<Arc<Image>>,
    cos: f32,
    sin: f32,
    x: f32,
    y: f32,
    half_w: f32,
    half_h: f32,
    shape: Shape,
    color: [f32; 3],
    opacity: f32,
    ops: Vec<Op>,
    vignette: f32,
    pixelate: f32,
    flip_x: bool,
    flip_y: bool,
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

pub fn text_image(text: &str, size: f32, color: [u8; 3]) -> Option<Image> {
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
        }
    }
    let image = match (image, radius as usize) {
        (Some(image), 0) => Some(image),
        (Some(image), radius) => Some(Arc::new(blur(&image, radius.min(64)))),
        (None, _) => None,
    };
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
        x: clip.x.at(frame) * scale,
        y: clip.y.at(frame) * scale,
        half_w: width * size / 2.0,
        half_h: height * size / 2.0,
        shape: clip.shape,
        opacity,
        ops,
        vignette,
        pixelate,
        flip_x,
        flip_y,
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
    if layer.pixelate > 1.0 {
        let (step_u, step_v) = (layer.pixelate / image.width.max(1) as f32, layer.pixelate / image.height.max(1) as f32);
        u = (u / step_u).floor() * step_u;
        v = (v / step_v).floor() * step_v;
    }
    let column = ((u * image.width as f32) as usize).min(image.width.saturating_sub(1));
    let row = ((v * image.height as f32) as usize).min(image.height.saturating_sub(1));
    let start = (row * image.width + column) * 4;
    let pixel = image.pixels.get(start..start + 4)?;
    let color = apply(&layer.ops, [pixel[0] as f32, pixel[1] as f32, pixel[2] as f32]);
    Some((color, pixel[3] as f32 / 255.0))
}

fn draw(layer: &Resolved, px: f32, py: f32) -> Option<([f32; 3], f32)> {
    let (dx, dy) = (px - layer.x, py - layer.y);
    let (lx, ly) = (dx * layer.cos - dy * layer.sin, dx * layer.sin + dy * layer.cos);
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
    if alpha <= 0.0 {
        return None;
    }
    let mut color = color;
    if layer.vignette > 0.0 {
        let (nx, ny) = (lx / layer.half_w, ly / layer.half_h);
        let falloff = 1.0 - layer.vignette * ((nx * nx + ny * ny) / 2.0).min(1.0);
        color = color.map(|channel| channel * falloff);
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
    let time = frame as f32;
    let mut order: Vec<&Clip> = project.clips.iter().filter(|clip| wanted_frame(project, clip, frame)).collect();
    order.sort_by_key(|clip| clip.track);
    let layers: Vec<Resolved> = order
        .into_iter()
        .filter_map(|clip| {
            let image = match (&clip.text, &clip.media) {
                (Some(text), _) => Some(Arc::new(text_image(text, clip.text_size, clip.color)?)),
                (None, Some(path)) => Some(images(path)?),
                (None, None) => None,
            };
            resolve(clip, time, scale, image)
        })
        .collect();
    let background = project.background.map(|channel| channel as f32);
    target.pixels.par_chunks_mut(width * 4).enumerate().for_each(|(row, line)| {
        let py = row as f32 + 0.5;
        for (column, pixel) in line.chunks_exact_mut(4).enumerate() {
            let px = column as f32 + 0.5;
            let mut color = background;
            for layer in &layers {
                if let Some((source, alpha)) = draw(layer, px, py) {
                    for index in 0..3 {
                        color[index] += (source[index] - color[index]) * alpha;
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
        let image = text_image("Hi", 40.0, [255, 255, 255]).unwrap();
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
