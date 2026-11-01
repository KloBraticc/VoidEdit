use std::sync::Arc;
use voidedit_core::{Effect, Project, EFFECT_NAMES};
use voidedit_media::{decode_now, FrameWant, Library};
use voidedit_render::{render, Frame};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (media, output) = (&args[0], &args[1]);
    let library = Library::default();
    let image = decode_now(&library, &FrameWant { path: media.clone(), seconds: 1.0, scale: 1.0 }).expect("frame");
    let (cell_w, cell_h, columns) = (240usize, 135usize, 7usize);
    let rows = EFFECT_NAMES.len().div_ceil(columns);
    let mut sheet = vec![255u8; cell_w * columns * cell_h * rows * 4];
    for (index, name) in EFFECT_NAMES.iter().enumerate() {
        let mut project = Project { width: cell_w as u32, height: cell_h as u32, ..Project::default() };
        let mut clip = project.new_media_clip(media, 0, Some((image.width as u32, image.height as u32)), 1.0);
        clip.effects.push(Effect::create(name).expect("effect"));
        project.clips.push(clip);
        let mut frame = Frame::default();
        let shared = image.clone();
        render(&project, 0, 1.0, &mut frame, &|_| Some(Arc::clone(&shared)));
        let (ox, oy) = (index % columns * cell_w, index / columns * cell_h);
        for y in 0..cell_h {
            let from = y * cell_w * 4;
            let to = ((oy + y) * cell_w * columns + ox) * 4;
            sheet[to..to + (cell_w - 2) * 4].copy_from_slice(&frame.pixels[from..from + (cell_w - 2) * 4]);
        }
    }
    image::save_buffer(output, &sheet, (cell_w * columns) as u32, (cell_h * rows) as u32, image::ColorType::Rgba8).expect("save");
    println!("{EFFECT_NAMES:?}");
}
