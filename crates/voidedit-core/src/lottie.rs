use serde_json::Value;

use crate::{Clip, Param, Project, Shape, Track, TrackKind};

pub struct LottieReport {
    pub project: Project,
    pub imported: usize,
    pub skipped: Vec<String>,
}

fn component(value: &Value, index: usize) -> Option<f32> {
    match value {
        Value::Array(items) => items.get(index).and_then(Value::as_f64).map(|number| number as f32),
        Value::Number(number) if index == 0 => number.as_f64().map(|number| number as f32),
        _ => None,
    }
}

fn apply(param: &mut Param, property: Option<&Value>, index: usize, map: &dyn Fn(f32) -> f32, shift: f32) {
    let Some(data) = property.and_then(|property| property.get("k")) else { return };
    if let Some(keys) = data.as_array().filter(|items| items.first().is_some_and(|item| item.get("t").is_some())) {
        for key in keys {
            let frame = key.get("t").and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as f32;
            if let Some(value) = key.get("s").and_then(|value| component(value, index)) {
                param.set_key(frame as u32, map(value) + shift);
            }
        }
        if let Some(value) = keys.first().and_then(|key| key.get("s")).and_then(|value| component(value, index)) {
            param.base = map(value) + shift;
        }
    } else if let Some(value) = component(data, index) {
        param.base = map(value) + shift;
    }
}

fn first_static(property: Option<&Value>, index: usize) -> Option<f32> {
    let data = property?.get("k")?;
    match data.as_array().and_then(|items| items.first()).filter(|item| item.get("t").is_some()) {
        Some(key) => key.get("s").and_then(|value| component(value, index)),
        None => component(data, index),
    }
}

struct Found {
    shape: Option<(Shape, f32, f32)>,
    color: Option<[u8; 3]>,
    path: bool,
}

fn search(items: &[Value], found: &mut Found) {
    for item in items {
        match item.get("ty").and_then(Value::as_str).unwrap_or("") {
            "gr" => search(item.get("it").and_then(Value::as_array).map_or(&[], Vec::as_slice), found),
            kind @ ("rc" | "el") if found.shape.is_none() => {
                let (width, height) = (first_static(item.get("s"), 0).unwrap_or(100.0), first_static(item.get("s"), 1).unwrap_or(100.0));
                found.shape = Some((if kind == "rc" { Shape::Rectangle } else { Shape::Ellipse }, width, height));
            }
            "sh" => found.path = true,
            "fl" if found.color.is_none() => {
                let channel = |index| (first_static(item.get("c"), index).unwrap_or(1.0).clamp(0.0, 1.0) * 255.0) as u8;
                found.color = Some([channel(0), channel(1), channel(2)]);
            }
            _ => {}
        }
    }
}

fn hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim_start_matches('#');
    Some([u8::from_str_radix(hex.get(0..2)?, 16).ok()?, u8::from_str_radix(hex.get(2..4)?, 16).ok()?, u8::from_str_radix(hex.get(4..6)?, 16).ok()?])
}

pub fn import(text: &str) -> Result<LottieReport, String> {
    let root: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let layers = root.get("layers").and_then(Value::as_array).ok_or("This is not a Lottie file, it has no layers")?;
    let number = |key: &str, fallback: f64| root.get(key).and_then(Value::as_f64).unwrap_or(fallback);
    let (start, end) = (number("ip", 0.0), number("op", 300.0));
    let mut project = Project { width: (number("w", 1920.0) as u32).max(16) & !1, height: (number("h", 1080.0) as u32).max(16) & !1, fps: (number("fr", 30.0).round() as u32).clamp(1, 240), duration: ((end - start) as u32).max(1), tracks: vec![Track::new("Audio 1", TrackKind::Audio)], ..Project::default() };
    let mut imported = 0;
    let mut skipped = Vec::new();
    for layer in layers.iter().rev() {
        let name = layer.get("nm").and_then(Value::as_str).unwrap_or("Layer").to_string();
        let in_frame = layer.get("ip").and_then(Value::as_f64).unwrap_or(start).max(0.0) as u32;
        let out_frame = layer.get("op").and_then(Value::as_f64).unwrap_or(end).max(1.0) as u32;
        let mut clip: Clip = match layer.get("ty").and_then(Value::as_i64).unwrap_or(-1) {
            1 => {
                let mut clip = project.new_clip(Shape::Solid, in_frame);
                clip.width = layer.get("sw").and_then(Value::as_f64).unwrap_or(project.width as f64) as f32;
                clip.height = layer.get("sh").and_then(Value::as_f64).unwrap_or(project.height as f64) as f32;
                if let Some(color) = layer.get("sc").and_then(Value::as_str).and_then(hex_color) {
                    clip.color = color;
                }
                clip
            }
            4 => {
                let mut found = Found { shape: None, color: None, path: false };
                search(layer.get("shapes").and_then(Value::as_array).map_or(&[], Vec::as_slice), &mut found);
                let Some((shape, width, height)) = found.shape else {
                    skipped.push(format!("{name}: {}", if found.path { "custom path shape" } else { "empty shape" }));
                    continue;
                };
                let mut clip = project.new_clip(shape, in_frame);
                clip.width = width;
                clip.height = height;
                clip.color = found.color.unwrap_or([255, 255, 255]);
                clip
            }
            5 => {
                let document = layer.get("t").and_then(|text| text.get("d")).and_then(|d| d.get("k")).and_then(Value::as_array).and_then(|keys| keys.first()).and_then(|key| key.get("s"));
                let Some(document) = document else {
                    skipped.push(format!("{name}: empty text"));
                    continue;
                };
                let mut clip = project.new_text(in_frame);
                clip.text = document.get("t").and_then(Value::as_str).map(|text| text.replace('\r', "\n"));
                clip.text_size = document.get("s").and_then(Value::as_f64).unwrap_or(96.0) as f32;
                if let Some(color) = document.get("fc").and_then(Value::as_array) {
                    let channel = |index: usize| (color.get(index).and_then(Value::as_f64).unwrap_or(1.0).clamp(0.0, 1.0) * 255.0) as u8;
                    clip.color = [channel(0), channel(1), channel(2)];
                }
                clip
            }
            0 => {
                skipped.push(format!("{name}: precomp"));
                continue;
            }
            2 => {
                skipped.push(format!("{name}: image"));
                continue;
            }
            _ => {
                skipped.push(format!("{name}: unsupported layer"));
                continue;
            }
        };
        project.tracks.push(Track::new(&format!("Video {}", project.tracks.len()), TrackKind::Video));
        clip.track = project.tracks.len() - 1;
        clip.name = name;
        clip.start = in_frame;
        clip.end = out_frame.max(in_frame + 1);
        let transform = layer.get("ks");
        let part = |key: &str| transform.and_then(|ks| ks.get(key));
        let identity = |value: f32| value;
        let scale = first_static(part("s"), 0).unwrap_or(100.0) / 100.0;
        let centered = |value: f32, half: f32| (half - value) * scale;
        let (anchor_x, anchor_y) = (first_static(part("a"), 0), first_static(part("a"), 1));
        let solid = layer.get("ty").and_then(Value::as_i64) != Some(4);
        let shift_x = anchor_x.map_or(0.0, |value| centered(value, if solid { clip.width / 2.0 } else { 0.0 }));
        let shift_y = anchor_y.map_or(0.0, |value| centered(value, if solid { clip.height / 2.0 } else { 0.0 }));
        clip.x = Param::new(0.0);
        clip.y = Param::new(0.0);
        apply(&mut clip.x, part("p"), 0, &identity, shift_x);
        apply(&mut clip.y, part("p"), 1, &identity, shift_y);
        apply(&mut clip.scale, part("s"), 0, &identity, 0.0);
        apply(&mut clip.rotation, part("r"), 0, &identity, 0.0);
        apply(&mut clip.opacity, part("o"), 0, &identity, 0.0);
        project.clips.push(clip);
        imported += 1;
    }
    Ok(LottieReport { project, imported, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_shapes_text_and_keyframes_and_reports_what_it_skips() {
        let source = r##"{"w":640,"h":360,"fr":24,"ip":0,"op":48,"layers":[
            {"ty":5,"nm":"Title","ip":0,"op":48,"ks":{"p":{"k":[320,180]}},"t":{"d":{"k":[{"s":{"t":"Hello","s":40,"fc":[1,0,0]},"t":0}]}}},
            {"ty":4,"nm":"Box","ip":0,"op":48,"ks":{"p":{"k":[{"t":0,"s":[0,0]},{"t":24,"s":[100,50]}]},"o":{"k":50}},"shapes":[{"ty":"gr","it":[{"ty":"rc","s":{"k":[20,10]}},{"ty":"fl","c":{"k":[0,1,0,1]}}]}]},
            {"ty":0,"nm":"Inner","ip":0,"op":48}
        ]}"##;
        let report = import(source).unwrap();
        assert_eq!((report.project.width, report.project.fps, report.imported), (640, 24, 2));
        assert_eq!(report.skipped.len(), 1);
        let boxed = report.project.clips.iter().find(|clip| clip.name == "Box").unwrap();
        assert_eq!(boxed.color, [0, 255, 0]);
        assert!((boxed.x.at(24.0) - 100.0).abs() < 0.01);
        assert!((boxed.opacity.at(0.0) - 50.0).abs() < 0.01);
        let title = report.project.clips.iter().find(|clip| clip.name == "Title").unwrap();
        assert_eq!(title.text.as_deref(), Some("Hello"));
    }

    #[test]
    fn rejects_files_that_are_not_lottie() {
        assert!(import("{}").is_err());
        assert!(import("not json").is_err());
    }
}
