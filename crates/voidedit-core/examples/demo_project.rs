use voidedit_core::{Effect, Project, Shape};

fn main() {
    let mut args = std::env::args().skip(1);
    let (media, output) = (args.next().unwrap_or_default(), args.next().unwrap_or_else(|| "demo.voidedit".into()));
    let mut project = Project::default();
    project.media.push(media.clone());
    let mut video = project.new_media_clip(&media, 0, Some((1280, 720)), 5.0);
    video.effects.push(Effect::Contrast { amount: voidedit_core::Param::new(20.0) });
    video.fade_in = 15;
    project.clips.push(video);
    let mut audio = project.new_media_clip(&media, 0, None, 5.0);
    audio.name = "Audio".into();
    project.clips.push(audio);
    let mut title = project.new_text(30);
    title.track = 2;
    title.end = 150;
    project.clips.push(title);
    let mut shape = project.new_clip(Shape::Ellipse, 60);
    shape.track = 3;
    project.clips.push(shape);
    project.markers.push(90);
    std::fs::write(output, project.to_json().unwrap_or_default()).unwrap_or_default();
}
