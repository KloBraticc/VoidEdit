use rayon::prelude::*;
use voidedit_core::{Clip, Effect, Project, Shape, TrackKind};

#[derive(Default)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

struct Resolved {
    cos: f32,
    sin: f32,
    x: f32,
    y: f32,
    half_w: f32,
    half_h: f32,
    shape: Shape,
    color: [f32; 3],
    opacity: f32,
}

fn resolve(layer: &Clip, frame: f32, scale: f32) -> Option<Resolved> {
    let opacity = layer.opacity.at(frame).clamp(0.0, 100.0) / 100.0;
    let size = layer.scale.at(frame).max(0.0) / 100.0 * scale;
    if opacity <= 0.0 || size <= 0.0 {
        return None;
    }
    let mut color = layer.color.map(|channel| channel as f32);
    for effect in &layer.effects {
        match effect {
            Effect::Brightness { amount } => {
                let gain = 1.0 + amount.at(frame) / 100.0;
                color = color.map(|channel| (channel * gain).clamp(0.0, 255.0));
            }
            Effect::Tint { amount, color: tint } => {
                let mix = amount.at(frame).clamp(0.0, 100.0) / 100.0;
                for index in 0..3 {
                    color[index] += (tint[index] as f32 - color[index]) * mix;
                }
            }
            Effect::Invert => color = color.map(|channel| 255.0 - channel),
        }
    }
    let (sin, cos) = (-layer.rotation.at(frame).to_radians()).sin_cos();
    Some(Resolved {
        cos,
        sin,
        x: layer.x.at(frame) * scale,
        y: layer.y.at(frame) * scale,
        half_w: layer.width * size / 2.0,
        half_h: layer.height * size / 2.0,
        shape: layer.shape,
        color,
        opacity,
    })
}

fn coverage(layer: &Resolved, px: f32, py: f32) -> f32 {
    let (dx, dy) = (px - layer.x, py - layer.y);
    let (lx, ly) = (dx * layer.cos - dy * layer.sin, dx * layer.sin + dy * layer.cos);
    if layer.half_w <= 0.0 || layer.half_h <= 0.0 {
        return 0.0;
    }
    match layer.shape {
        Shape::Solid | Shape::Rectangle => {
            let edge = (layer.half_w - lx.abs()).min(layer.half_h - ly.abs());
            (edge + 0.5).clamp(0.0, 1.0)
        }
        Shape::Ellipse => {
            let (nx, ny) = (lx / layer.half_w, ly / layer.half_h);
            let distance = (nx * nx + ny * ny).sqrt();
            let edge = (1.0 - distance) * layer.half_w.min(layer.half_h);
            (edge + 0.5).clamp(0.0, 1.0)
        }
    }
}

pub fn render(project: &Project, frame: u32, scale: f32, target: &mut Frame) {
    let scale = scale.clamp(0.05, 1.0);
    let width = ((project.width as f32 * scale) as usize).max(1);
    let height = ((project.height as f32 * scale) as usize).max(1);
    target.width = width;
    target.height = height;
    target.pixels.resize(width * height * 4, 255);
    let time = frame as f32;
    let mut order: Vec<&Clip> = project
        .clips
        .iter()
        .filter(|clip| clip.media.is_none() && clip.visible && frame >= clip.start && frame < clip.end)
        .filter(|clip| project.tracks.get(clip.track).is_some_and(|track| track.kind == TrackKind::Video))
        .collect();
    order.sort_by_key(|clip| clip.track);
    let layers: Vec<Resolved> = order.into_iter().filter_map(|clip| resolve(clip, time, scale)).collect();
    let background = project.background.map(|channel| channel as f32);
    target.pixels.par_chunks_mut(width * 4).enumerate().for_each(|(row, line)| {
        let py = row as f32 + 0.5;
        for (column, pixel) in line.chunks_exact_mut(4).enumerate() {
            let px = column as f32 + 0.5;
            let mut color = background;
            for layer in &layers {
                let alpha = coverage(layer, px, py) * layer.opacity;
                if alpha > 0.0 {
                    for index in 0..3 {
                        color[index] += (layer.color[index] - color[index]) * alpha;
                    }
                }
            }
            pixel[0] = color[0] as u8;
            pixel[1] = color[1] as u8;
            pixel[2] = color[2] as u8;
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
        let mut frame = Frame::default();
        render(&project, 0, 1.0, &mut frame);
        assert_eq!(&frame.pixels[..4], &[200, 100, 50, 255]);
    }

    #[test]
    fn out_of_range_clips_do_not_draw() {
        let mut project = small();
        let clip = project.new_clip(Shape::Solid, 5);
        project.clips.push(clip);
        let mut frame = Frame::default();
        render(&project, 0, 1.0, &mut frame);
        assert_eq!(&frame.pixels[..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn zero_size_shapes_do_not_crash() {
        let mut project = small();
        let mut clip = project.new_clip(Shape::Ellipse, 0);
        clip.width = 0.0;
        project.clips.push(clip);
        render(&project, 0, 1.0, &mut Frame::default());
    }
}
