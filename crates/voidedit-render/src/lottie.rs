use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use tiny_skia::{BlendMode, Color, FillRule, FilterQuality, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Point, RadialGradient, Shader, SpreadMode, Stroke, Transform};

use crate::{styled_text_image, Image};

const MAX_DEPTH: u32 = 12;
const KAPPA: f32 = 0.552_284_8;

enum Asset {
    Comp(Vec<Value>),
    Picture(Pixmap),
}

pub struct Animation {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub start: f32,
    pub end: f32,
    layers: Vec<Value>,
    assets: HashMap<String, Asset>,
    pub warnings: Vec<String>,
}

fn decode_base64(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

fn picture(bytes: &[u8]) -> Option<Pixmap> {
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let mut pixmap = Pixmap::new(image.width(), image.height())?;
    for (target, source) in pixmap.pixels_mut().iter_mut().zip(image.pixels()) {
        let [r, g, b, a] = source.0;
        *target = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(pixmap)
}

fn scan(value: &Value, warnings: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.get("x").is_some_and(Value::is_string) && map.contains_key("k") {
                warnings.push("expressions".to_string());
            }
            if map.get("ef").and_then(Value::as_array).is_some_and(|effects| !effects.is_empty()) {
                warnings.push("layer effects".to_string());
            }
            if map.get("sy").and_then(Value::as_array).is_some_and(|styles| !styles.is_empty()) {
                warnings.push("layer styles".to_string());
            }
            if map.get("ddd").and_then(Value::as_i64) == Some(1) {
                warnings.push("3D layers".to_string());
            }
            for item in map.values() {
                scan(item, warnings);
            }
        }
        Value::Array(items) => {
            for item in items {
                scan(item, warnings);
            }
        }
        _ => {}
    }
}

fn read_source(path: &str) -> Result<(String, HashMap<String, Vec<u8>>), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if !bytes.starts_with(b"PK") {
        return String::from_utf8(bytes).map(|text| (text, HashMap::new())).map_err(|_| "This file is not text or a .lottie package".to_string());
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let mut json = None;
    let mut files = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(|error| error.to_string())?;
        if name.ends_with(".json") && name.contains("animations/") && json.is_none() {
            json = String::from_utf8(data).ok();
        } else if let Some(file) = Path::new(&name).file_name() {
            files.insert(file.to_string_lossy().into_owned(), data);
        }
    }
    json.map(|text| (text, files)).ok_or_else(|| "This .lottie package has no animation".to_string())
}

fn parse(path: &str) -> Result<Animation, String> {
    let (text, files) = read_source(path)?;
    let root: Value = serde_json::from_str(&text).map_err(|error| format!("This is not valid Lottie JSON: {error}"))?;
    let layers = root.get("layers").and_then(Value::as_array).cloned().ok_or("This is not a Lottie file, it has no layers")?;
    let number = |key: &str, fallback: f64| root.get(key).and_then(Value::as_f64).unwrap_or(fallback);
    let mut assets = HashMap::new();
    let folder = Path::new(path).parent().map(Path::to_path_buf);
    for asset in root.get("assets").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = asset.get("id").and_then(Value::as_str) else { continue };
        if let Some(inner) = asset.get("layers").and_then(Value::as_array) {
            assets.insert(id.to_string(), Asset::Comp(inner.clone()));
        } else if let Some(name) = asset.get("p").and_then(Value::as_str) {
            let data = if let Some((_, encoded)) = name.split_once("base64,") {
                Some(decode_base64(encoded))
            } else {
                let base = asset.get("u").and_then(Value::as_str).unwrap_or("");
                files.get(name).cloned().or_else(|| folder.as_ref().and_then(|folder| std::fs::read(folder.join(base).join(name)).ok()))
            };
            if let Some(image) = data.as_deref().and_then(picture) {
                assets.insert(id.to_string(), Asset::Picture(image));
            }
        }
    }
    let mut warnings = Vec::new();
    scan(&root, &mut warnings);
    warnings.sort();
    warnings.dedup();
    Ok(Animation { width: (number("w", 1920.0) as u32).clamp(2, 8192), height: (number("h", 1080.0) as u32).clamp(2, 8192), fps: number("fr", 30.0).max(1.0) as f32, start: number("ip", 0.0) as f32, end: number("op", 300.0) as f32, layers, assets, warnings })
}

pub fn load(path: &str) -> Result<Arc<Animation>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Animation>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|cache| cache.get(path).cloned()) {
        return Ok(found);
    }
    let animation = Arc::new(parse(path)?);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(path.to_string(), animation.clone());
    }
    Ok(animation)
}

fn bezier_ease(key: &Value, x: f32) -> f32 {
    let scalar = |value: Option<&Value>| match value {
        Some(Value::Array(items)) => items.first().and_then(Value::as_f64).map(|v| v as f32),
        Some(Value::Number(number)) => number.as_f64().map(|v| v as f32),
        _ => None,
    };
    let (Some(out), Some(into)) = (key.get("o"), key.get("i")) else { return x };
    let (Some(x1), Some(y1), Some(x2), Some(y2)) = (scalar(out.get("x")), scalar(out.get("y")), scalar(into.get("x")), scalar(into.get("y"))) else { return x };
    let at = |a: f32, b: f32, t: f32| 3.0 * (1.0 - t) * (1.0 - t) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..18 {
        let middle = (low + high) / 2.0;
        if at(x1, x2, middle) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    at(y1, y2, (low + high) / 2.0)
}

fn numbers(value: &Value) -> Vec<f32> {
    match value {
        Value::Number(number) => vec![number.as_f64().unwrap_or(0.0) as f32],
        Value::Array(items) => items.iter().filter_map(Value::as_f64).map(|v| v as f32).collect(),
        _ => Vec::new(),
    }
}

fn segment<'a>(keys: &'a [Value], time: f32) -> (&'a Value, Option<&'a Value>, f32) {
    let frame = |key: &Value| key.get("t").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let first = &keys[0];
    if time <= frame(first) {
        return (first, None, 0.0);
    }
    for pair in keys.windows(2) {
        let (from, to) = (frame(&pair[0]), frame(&pair[1]));
        if time < to {
            if pair[0].get("h").and_then(Value::as_i64) == Some(1) {
                return (&pair[0], None, 0.0);
            }
            return (&pair[0], Some(&pair[1]), bezier_ease(&pair[0], (time - from) / (to - from).max(0.001)));
        }
    }
    let last = &keys[keys.len() - 1];
    if last.get("s").is_some() {
        (last, None, 0.0)
    } else if keys.len() >= 2 {
        (&keys[keys.len() - 2], Some(last), 1.0)
    } else {
        (last, None, 0.0)
    }
}

fn is_keyed(data: &Value) -> bool {
    data.as_array().is_some_and(|items| items.first().is_some_and(|item| item.is_object() && item.get("t").is_some()))
}

fn value(property: Option<&Value>, time: f32) -> Vec<f32> {
    let Some(data) = property.and_then(|property| property.get("k")) else { return Vec::new() };
    if !is_keyed(data) {
        return numbers(data);
    }
    let keys = data.as_array().map_or(&[][..], Vec::as_slice);
    let (key, next, amount) = segment(keys, time);
    let start = key.get("s").map(numbers).unwrap_or_default();
    let Some(next) = next else { return start };
    let end = key.get("e").or_else(|| next.get("s")).map(numbers).unwrap_or_else(|| start.clone());
    if start.is_empty() {
        return end;
    }
    start.iter().enumerate().map(|(index, from)| from + (end.get(index).copied().unwrap_or(*from) - from) * amount).collect()
}

fn scalar(property: Option<&Value>, time: f32, fallback: f32) -> f32 {
    value(property, time).first().copied().unwrap_or(fallback)
}

fn pair(property: Option<&Value>, time: f32, fallback: (f32, f32)) -> (f32, f32) {
    if property.and_then(|property| property.get("s")).and_then(Value::as_bool) == Some(true) {
        let property = property.unwrap_or(&Value::Null);
        return (scalar(property.get("x"), time, fallback.0), scalar(property.get("y"), time, fallback.1));
    }
    let list = value(property, time);
    (list.first().copied().unwrap_or(fallback.0), list.get(1).copied().unwrap_or(fallback.1))
}

fn transform_of(data: Option<&Value>, time: f32) -> (Transform, f32) {
    let Some(data) = data else { return (Transform::identity(), 1.0) };
    let (anchor_x, anchor_y) = pair(data.get("a"), time, (0.0, 0.0));
    let (x, y) = pair(data.get("p"), time, (0.0, 0.0));
    let (scale_x, scale_y) = pair(data.get("s"), time, (100.0, 100.0));
    let rotation = scalar(data.get("r").or_else(|| data.get("rz")), time, 0.0);
    let matrix = Transform::from_translate(x, y).pre_concat(Transform::from_rotate(rotation)).pre_scale(scale_x / 100.0, scale_y / 100.0).pre_translate(-anchor_x, -anchor_y);
    (matrix, scalar(data.get("o"), time, 100.0) / 100.0)
}

struct Bezier {
    points: Vec<[f32; 2]>,
    into: Vec<[f32; 2]>,
    out: Vec<[f32; 2]>,
    closed: bool,
}

fn bezier_from(shape: &Value) -> Option<Bezier> {
    let list = |key: &str| -> Option<Vec<[f32; 2]>> { Some(shape.get(key)?.as_array()?.iter().map(|point| { let values = numbers(point); [values.first().copied().unwrap_or(0.0), values.get(1).copied().unwrap_or(0.0)] }).collect()) };
    Some(Bezier { points: list("v")?, into: list("i")?, out: list("o")?, closed: shape.get("c").and_then(Value::as_bool).unwrap_or(false) })
}

fn bezier_at(property: Option<&Value>, time: f32) -> Option<Bezier> {
    let data = property?.get("k")?;
    if !is_keyed(data) {
        return bezier_from(data);
    }
    let keys = data.as_array()?;
    let (key, next, amount) = segment(keys, time);
    let first = |item: &Value, name: &str| item.get(name).and_then(Value::as_array).and_then(|items| items.first()).and_then(bezier_from);
    let start = first(key, "s");
    let Some(next) = next else { return start };
    let end = first(key, "e").or_else(|| first(next, "s"));
    let (Some(mut start), Some(end)) = (start, end) else { return None };
    let mix = |from: &mut [[f32; 2]], to: &[[f32; 2]]| {
        for (a, b) in from.iter_mut().zip(to) {
            a[0] += (b[0] - a[0]) * amount;
            a[1] += (b[1] - a[1]) * amount;
        }
    };
    mix(&mut start.points, &end.points);
    mix(&mut start.into, &end.into);
    mix(&mut start.out, &end.out);
    Some(start)
}

fn add_bezier(builder: &mut PathBuilder, path: &Bezier) {
    let Some(first) = path.points.first() else { return };
    builder.move_to(first[0], first[1]);
    let count = path.points.len();
    let limit = if path.closed { count } else { count.saturating_sub(1) };
    for index in 0..limit {
        let next = (index + 1) % count;
        let (a, b) = (path.points[index], path.points[next]);
        let (o, i) = (path.out.get(index).copied().unwrap_or([0.0; 2]), path.into.get(next).copied().unwrap_or([0.0; 2]));
        builder.cubic_to(a[0] + o[0], a[1] + o[1], b[0] + i[0], b[1] + i[1], b[0], b[1]);
    }
    if path.closed {
        builder.close();
    }
}

fn shape_path(item: &Value, kind: &str, time: f32) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    match kind {
        "sh" => add_bezier(&mut builder, &bezier_at(item.get("ks"), time)?),
        "rc" => {
            let (x, y) = pair(item.get("p"), time, (0.0, 0.0));
            let (w, h) = pair(item.get("s"), time, (0.0, 0.0));
            let round = scalar(item.get("r"), time, 0.0).min(w / 2.0).min(h / 2.0).max(0.0);
            let (left, top, right, bottom) = (x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0);
            let c = round * (1.0 - KAPPA);
            builder.move_to(left + round, top);
            builder.line_to(right - round, top);
            builder.cubic_to(right - c, top, right, top + c, right, top + round);
            builder.line_to(right, bottom - round);
            builder.cubic_to(right, bottom - c, right - c, bottom, right - round, bottom);
            builder.line_to(left + round, bottom);
            builder.cubic_to(left + c, bottom, left, bottom - c, left, bottom - round);
            builder.line_to(left, top + round);
            builder.cubic_to(left, top + c, left + c, top, left + round, top);
            builder.close();
        }
        "el" => {
            let (x, y) = pair(item.get("p"), time, (0.0, 0.0));
            let (w, h) = pair(item.get("s"), time, (0.0, 0.0));
            let (rx, ry) = (w / 2.0, h / 2.0);
            let (kx, ky) = (rx * KAPPA, ry * KAPPA);
            builder.move_to(x, y - ry);
            builder.cubic_to(x + kx, y - ry, x + rx, y - ky, x + rx, y);
            builder.cubic_to(x + rx, y + ky, x + kx, y + ry, x, y + ry);
            builder.cubic_to(x - kx, y + ry, x - rx, y + ky, x - rx, y);
            builder.cubic_to(x - rx, y - ky, x - kx, y - ry, x, y - ry);
            builder.close();
        }
        "sr" => {
            let (x, y) = pair(item.get("p"), time, (0.0, 0.0));
            let points = scalar(item.get("pt"), time, 5.0).max(3.0) as usize;
            let outer = scalar(item.get("or"), time, 0.0);
            let star = item.get("sy").and_then(Value::as_i64).unwrap_or(1) == 1;
            let inner = scalar(item.get("ir"), time, outer * 0.5);
            let rotation = scalar(item.get("r"), time, 0.0).to_radians() - std::f32::consts::FRAC_PI_2;
            let steps = if star { points * 2 } else { points };
            for step in 0..steps {
                let radius = if star && step % 2 == 1 { inner } else { outer };
                let angle = rotation + step as f32 / steps as f32 * std::f32::consts::TAU;
                let (px, py) = (x + radius * angle.cos(), y + radius * angle.sin());
                if step == 0 {
                    builder.move_to(px, py);
                } else {
                    builder.line_to(px, py);
                }
            }
            builder.close();
        }
        _ => return None,
    }
    builder.finish()
}

fn trim_path(path: &tiny_skia::Path, trim: (f32, f32, f32)) -> Option<tiny_skia::Path> {
    let (mut start, mut end, offset) = trim;
    start = (start + offset / 3.6).rem_euclid(100.001) / 100.0;
    end = (end + offset / 3.6).rem_euclid(100.001) / 100.0;
    if (trim.1 - trim.0).abs() >= 100.0 {
        return Some(path.clone());
    }
    if (trim.1 - trim.0).abs() < 0.001 {
        return None;
    }
    let mut lines: Vec<Vec<Point>> = Vec::new();
    let mut last = Point::zero();
    let mut first = Point::zero();
    const STEPS: usize = 24;
    for piece in path.segments() {
        match piece {
            tiny_skia::PathSegment::MoveTo(point) => {
                lines.push(vec![point]);
                last = point;
                first = point;
            }
            tiny_skia::PathSegment::LineTo(point) => {
                if let Some(line) = lines.last_mut() {
                    line.push(point);
                }
                last = point;
            }
            tiny_skia::PathSegment::QuadTo(a, b) => {
                for step in 1..=STEPS {
                    let t = step as f32 / STEPS as f32;
                    let u = 1.0 - t;
                    let point = Point::from_xy(u * u * last.x + 2.0 * u * t * a.x + t * t * b.x, u * u * last.y + 2.0 * u * t * a.y + t * t * b.y);
                    if let Some(line) = lines.last_mut() {
                        line.push(point);
                    }
                }
                last = b;
            }
            tiny_skia::PathSegment::CubicTo(a, b, c) => {
                for step in 1..=STEPS {
                    let t = step as f32 / STEPS as f32;
                    let u = 1.0 - t;
                    let point = Point::from_xy(u * u * u * last.x + 3.0 * u * u * t * a.x + 3.0 * u * t * t * b.x + t * t * t * c.x, u * u * u * last.y + 3.0 * u * u * t * a.y + 3.0 * u * t * t * b.y + t * t * t * c.y);
                    if let Some(line) = lines.last_mut() {
                        line.push(point);
                    }
                }
                last = c;
            }
            tiny_skia::PathSegment::Close => {
                if let Some(line) = lines.last_mut() {
                    line.push(first);
                }
                last = first;
            }
        }
    }
    let distance = |a: Point, b: Point| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let total: f32 = lines.iter().map(|line| line.windows(2).map(|pair| distance(pair[0], pair[1])).sum::<f32>()).sum();
    if total <= 0.0 {
        return None;
    }
    let ranges: Vec<(f32, f32)> = if start <= end { vec![(start * total, end * total)] } else { vec![(start * total, total), (0.0, end * total)] };
    let mut builder = PathBuilder::new();
    for (from, to) in ranges {
        let mut walked = 0.0;
        for line in &lines {
            let mut pen = false;
            for pair in line.windows(2) {
                let length = distance(pair[0], pair[1]);
                let (begin, finish) = (walked, walked + length);
                walked = finish;
                if finish < from || begin > to || length <= 0.0 {
                    pen = false;
                    continue;
                }
                let point = |at: f32| {
                    let t = ((at - begin) / length).clamp(0.0, 1.0);
                    (pair[0].x + (pair[1].x - pair[0].x) * t, pair[0].y + (pair[1].y - pair[0].y) * t)
                };
                let a = point(from.max(begin));
                let b = point(to.min(finish));
                if !pen {
                    builder.move_to(a.0, a.1);
                    pen = true;
                }
                builder.line_to(b.0, b.1);
            }
        }
    }
    builder.finish()
}

fn gradient_shader(item: &Value, time: f32, alpha: f32) -> Option<Shader<'static>> {
    let info = item.get("g")?;
    let count = info.get("p").and_then(Value::as_i64)?.max(2) as usize;
    let data = value(info.get("k"), time);
    let mut stops = Vec::with_capacity(count);
    for index in 0..count {
        let start = index * 4;
        let (position, r, g, b) = (*data.get(start)?, *data.get(start + 1)?, *data.get(start + 2)?, *data.get(start + 3)?);
        let opacity = data.get(count * 4 + index * 2 + 1).copied().unwrap_or(1.0);
        stops.push(GradientStop::new(position, Color::from_rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), (opacity * alpha).clamp(0.0, 1.0))?));
    }
    let (sx, sy) = pair(item.get("s"), time, (0.0, 0.0));
    let (ex, ey) = pair(item.get("e"), time, (0.0, 0.0));
    if item.get("t").and_then(Value::as_i64) == Some(2) {
        let radius = ((ex - sx).powi(2) + (ey - sy).powi(2)).sqrt().max(0.01);
        RadialGradient::new(Point::from_xy(sx, sy), Point::from_xy(sx, sy), radius, stops, SpreadMode::Pad, Transform::identity())
    } else {
        LinearGradient::new(Point::from_xy(sx, sy), Point::from_xy(ex, ey), stops, SpreadMode::Pad, Transform::identity())
    }
}

fn draw_shapes(items: &[Value], inherited: &[&Value], trims: &[(f32, f32, f32)], time: f32, base: Transform, opacity: f32, target: &mut Pixmap) {
    let mut painters: Vec<&Value> = inherited.to_vec();
    let mut trim_list: Vec<(f32, f32, f32)> = trims.to_vec();
    for item in items {
        if item.get("ty").and_then(Value::as_str) == Some("tm") {
            trim_list.push((scalar(item.get("s"), time, 0.0), scalar(item.get("e"), time, 100.0), scalar(item.get("o"), time, 0.0)));
        }
    }
    for item in items.iter().rev() {
        if item.get("hd").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let kind = item.get("ty").and_then(Value::as_str).unwrap_or("");
        match kind {
            "fl" | "st" | "gf" | "gs" => painters.push(item),
            "gr" => {
                let inner = item.get("it").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
                let group = inner.iter().find(|child| child.get("ty").and_then(Value::as_str) == Some("tr"));
                let (matrix, alpha) = transform_of(group, time);
                draw_shapes(inner, &painters, &trim_list, time, base.pre_concat(matrix), opacity * alpha, target);
            }
            "sh" | "rc" | "el" | "sr" => {
                let Some(mut path) = shape_path(item, kind, time) else { continue };
                let mut missing = false;
                for trim in &trim_list {
                    match trim_path(&path, *trim) {
                        Some(cut) => path = cut,
                        None => {
                            missing = true;
                            break;
                        }
                    }
                }
                if missing {
                    continue;
                }
                for painter in painters.iter().rev() {
                    paint_path(painter, &path, time, base, opacity, target);
                }
            }
            _ => {}
        }
    }
}

fn paint_path(painter: &Value, path: &tiny_skia::Path, time: f32, base: Transform, opacity: f32, target: &mut Pixmap) {
    let kind = painter.get("ty").and_then(Value::as_str).unwrap_or("");
    let alpha = (scalar(painter.get("o"), time, 100.0) / 100.0 * opacity).clamp(0.0, 1.0);
    let shader = if kind == "fl" || kind == "st" {
        let color = value(painter.get("c"), time);
        let channel = |index: usize| color.get(index).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        Color::from_rgba(channel(0), channel(1), channel(2), alpha).map(Shader::SolidColor)
    } else {
        gradient_shader(painter, time, alpha)
    };
    let Some(shader) = shader else { return };
    let paint = Paint { shader, anti_alias: true, ..Paint::default() };
    if kind == "fl" || kind == "gf" {
        let rule = if painter.get("r").and_then(Value::as_i64) == Some(2) { FillRule::EvenOdd } else { FillRule::Winding };
        target.fill_path(path, &paint, rule, base, None);
    } else {
        let line_cap = match painter.get("lc").and_then(Value::as_i64) {
            Some(2) => LineCap::Round,
            Some(3) => LineCap::Square,
            _ => LineCap::Butt,
        };
        let line_join = match painter.get("lj").and_then(Value::as_i64) {
            Some(2) => LineJoin::Round,
            Some(3) => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
        let stroke = Stroke { width: scalar(painter.get("w"), time, 1.0).max(0.0), line_cap, line_join, miter_limit: painter.get("ml").and_then(Value::as_f64).unwrap_or(4.0) as f32, dash: None };
        target.stroke_path(path, &paint, &stroke, base, None);
    }
}

fn world(layers: &[Value], layer: &Value, time: f32, depth: u32) -> Transform {
    let (matrix, _) = transform_of(layer.get("ks"), time);
    let parent = layer.get("parent").and_then(Value::as_i64).and_then(|id| layers.iter().find(|item| item.get("ind").and_then(Value::as_i64) == Some(id)));
    match parent {
        Some(parent) if depth < MAX_DEPTH => world(layers, parent, time, depth + 1).pre_concat(matrix),
        _ => matrix,
    }
}

fn apply_masks(layer: &Value, time: f32, matrix: Transform, content: &mut Pixmap) {
    let Some(masks) = layer.get("masksProperties").and_then(Value::as_array).filter(|masks| !masks.is_empty()) else { return };
    let (width, height) = (content.width(), content.height());
    let first_subtract = masks.first().and_then(|mask| mask.get("mode")).and_then(Value::as_str) == Some("s");
    let mut alpha = vec![if first_subtract { 255u8 } else { 0u8 }; (width * height) as usize];
    for mask in masks {
        let mode = mask.get("mode").and_then(Value::as_str).unwrap_or("a");
        if mode == "n" {
            continue;
        }
        let Some(bezier) = bezier_at(mask.get("pt"), time) else { continue };
        let mut builder = PathBuilder::new();
        add_bezier(&mut builder, &Bezier { closed: true, ..bezier });
        let Some(path) = builder.finish() else { continue };
        let Some(mut shape) = Mask::new(width, height) else { continue };
        shape.fill_path(&path, FillRule::Winding, true, matrix);
        let strength = scalar(mask.get("o"), time, 100.0) / 100.0;
        let inverted = mask.get("inv").and_then(Value::as_bool) == Some(true);
        for (out, value) in alpha.iter_mut().zip(shape.data()) {
            let mut amount = *value as f32 * strength;
            if inverted {
                amount = 255.0 - amount;
            }
            let amount = amount as u8;
            *out = match mode {
                "s" => (*out).min(255 - amount),
                "i" => (*out).min(amount),
                _ => (*out).max(amount),
            };
        }
    }
    multiply_alpha(content, &alpha);
}

fn multiply_alpha(content: &mut Pixmap, alpha: &[u8]) {
    for (pixel, amount) in content.data_mut().chunks_exact_mut(4).zip(alpha) {
        for channel in pixel {
            *channel = (*channel as u32 * *amount as u32 / 255) as u8;
        }
    }
}

fn blend_of(layer: &Value) -> BlendMode {
    match layer.get("bm").and_then(Value::as_i64).unwrap_or(0) {
        1 => BlendMode::Multiply,
        2 => BlendMode::Screen,
        3 => BlendMode::Overlay,
        4 => BlendMode::Darken,
        5 => BlendMode::Lighten,
        6 => BlendMode::ColorDodge,
        7 => BlendMode::ColorBurn,
        8 => BlendMode::HardLight,
        9 => BlendMode::SoftLight,
        10 => BlendMode::Difference,
        11 => BlendMode::Exclusion,
        12 => BlendMode::Hue,
        13 => BlendMode::Saturation,
        14 => BlendMode::Color,
        15 => BlendMode::Luminosity,
        _ => BlendMode::SourceOver,
    }
}

impl Animation {
    fn content(&self, layer: &Value, time: f32, matrix: Transform, target: &mut Pixmap, depth: u32) {
        match layer.get("ty").and_then(Value::as_i64).unwrap_or(-1) {
            0 => {
                let Some(Asset::Comp(inner)) = layer.get("refId").and_then(Value::as_str).and_then(|id| self.assets.get(id)) else { return };
                let local = match layer.get("tm") {
                    Some(remap) => scalar(Some(remap), time, 0.0) * self.fps,
                    None => time,
                };
                if depth < MAX_DEPTH {
                    self.layers_into(inner, local, matrix, target, depth + 1);
                }
            }
            1 => {
                let (width, height) = (layer.get("sw").and_then(Value::as_f64).unwrap_or(0.0) as f32, layer.get("sh").and_then(Value::as_f64).unwrap_or(0.0) as f32);
                let color = layer.get("sc").and_then(Value::as_str).unwrap_or("#000000").trim_start_matches('#').to_string();
                let channel = |at: usize| color.get(at..at + 2).and_then(|hex| u8::from_str_radix(hex, 16).ok()).unwrap_or(0);
                let mut paint = Paint::default();
                paint.set_color_rgba8(channel(0), channel(2), channel(4), 255);
                if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, width.max(0.01), height.max(0.01)) {
                    target.fill_rect(rect, &paint, matrix, None);
                }
            }
            2 => {
                if let Some(Asset::Picture(image)) = layer.get("refId").and_then(Value::as_str).and_then(|id| self.assets.get(id)) {
                    target.draw_pixmap(0, 0, image.as_ref(), &PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() }, matrix, None);
                }
            }
            4 => {
                let items = layer.get("shapes").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
                draw_shapes(items, &[], &[], time, matrix, 1.0, target);
            }
            5 => {
                let Some(document) = layer.get("t").and_then(|text| text.get("d")).and_then(|d| d.get("k")).and_then(Value::as_array) else { return };
                let current = document.iter().take_while(|key| key.get("t").and_then(Value::as_f64).unwrap_or(0.0) as f32 <= time).last().or(document.first()).and_then(|key| key.get("s"));
                let Some(current) = current else { return };
                let text = current.get("t").and_then(Value::as_str).unwrap_or("").replace('\r', "\n");
                let size = current.get("s").and_then(Value::as_f64).unwrap_or(48.0) as f32;
                let color = current.get("fc").map(numbers).unwrap_or_default();
                let channel = |at: usize| (color.get(at).copied().unwrap_or(1.0).clamp(0.0, 1.0) * 255.0) as u8;
                let Some(image) = styled_text_image(&text, size, [channel(0), channel(1), channel(2)], 0.0, false, &voidedit_core::TextStyle::default(), 0.0) else { return };
                let Some(rendered) = pixmap_from(&image) else { return };
                let justify = current.get("j").and_then(Value::as_i64).unwrap_or(0);
                let x = match justify {
                    1 => -(image.width as f32),
                    2 => -(image.width as f32) / 2.0,
                    _ => 0.0,
                };
                target.draw_pixmap(0, 0, rendered.as_ref(), &PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() }, matrix.pre_translate(x, -size), None);
            }
            _ => {}
        }
    }

    fn layer_visible(layer: &Value, time: f32) -> bool {
        let start = layer.get("ip").and_then(Value::as_f64).unwrap_or(f64::MIN) as f32;
        let end = layer.get("op").and_then(Value::as_f64).unwrap_or(f64::MAX) as f32;
        time >= start && time < end && layer.get("hd").and_then(Value::as_bool) != Some(true)
    }

    fn layers_into(&self, layers: &[Value], time: f32, base: Transform, target: &mut Pixmap, depth: u32) {
        let (width, height) = (target.width(), target.height());
        for (index, layer) in layers.iter().enumerate().rev() {
            if layer.get("td").and_then(Value::as_i64).unwrap_or(0) != 0 || !Self::layer_visible(layer, time) {
                continue;
            }
            let start = layer.get("st").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let stretch = layer.get("sr").and_then(Value::as_f64).unwrap_or(1.0).max(0.001) as f32;
            let local = (time - start) / stretch;
            let matrix = base.pre_concat(world(layers, layer, local, 0));
            let (_, opacity) = transform_of(layer.get("ks"), local);
            let Some(mut content) = Pixmap::new(width, height) else { continue };
            self.content(layer, local, matrix, &mut content, depth);
            apply_masks(layer, local, matrix, &mut content);
            if let Some(mode) = layer.get("tt").and_then(Value::as_i64).filter(|mode| *mode != 0) {
                let source = layer.get("tp").and_then(Value::as_i64).and_then(|id| layers.iter().find(|item| item.get("ind").and_then(Value::as_i64) == Some(id))).or_else(|| index.checked_sub(1).and_then(|previous| layers.get(previous)));
                if let (Some(source), Some(mut matte)) = (source, Pixmap::new(width, height)) {
                    let source_start = source.get("st").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    let source_time = time - source_start;
                    let source_matrix = base.pre_concat(world(layers, source, source_time, 0));
                    if Self::layer_visible(source, time) {
                        self.content(source, source_time, source_matrix, &mut matte, depth);
                        apply_masks(source, source_time, source_matrix, &mut matte);
                    }
                    let strength = transform_of(source.get("ks"), source_time).1;
                    let alpha: Vec<u8> = matte.data().chunks_exact(4).map(|pixel| {
                        let amount = match mode {
                            2 => 255.0 - pixel[3] as f32 * strength,
                            3 => (pixel[0] as f32 * 0.3 + pixel[1] as f32 * 0.59 + pixel[2] as f32 * 0.11) * strength,
                            4 => 255.0 - (pixel[0] as f32 * 0.3 + pixel[1] as f32 * 0.59 + pixel[2] as f32 * 0.11) * strength,
                            _ => pixel[3] as f32 * strength,
                        };
                        amount.clamp(0.0, 255.0) as u8
                    }).collect();
                    multiply_alpha(&mut content, &alpha);
                }
            }
            target.draw_pixmap(0, 0, content.as_ref(), &PixmapPaint { opacity: opacity.clamp(0.0, 1.0), blend_mode: blend_of(layer), quality: FilterQuality::Nearest }, Transform::identity(), None);
        }
    }

    pub fn render(&self, frame: f32, scale: f32) -> Option<Image> {
        let width = ((self.width as f32 * scale) as u32).max(1);
        let height = ((self.height as f32 * scale) as u32).max(1);
        let mut target = Pixmap::new(width, height)?;
        self.layers_into(&self.layers, frame + self.start, Transform::from_scale(scale, scale), &mut target, 0);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for pixel in target.pixels() {
            let color = pixel.demultiply();
            pixels.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
        }
        Some(Image { width: width as usize, height: height as usize, pixels })
    }
}

fn pixmap_from(image: &Image) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(image.width as u32, image.height as u32)?;
    for (target, source) in pixmap.pixels_mut().iter_mut().zip(image.pixels.chunks_exact(4)) {
        *target = tiny_skia::ColorU8::from_rgba(source[0], source[1], source[2], source[3]).premultiply();
    }
    Some(pixmap)
}

pub fn warnings(path: &str) -> Vec<String> {
    match voidedit_fx::lottie::engine(path) {
        Ok(engine) => engine.warnings.clone(),
        Err(error) => vec![format!("the full engine could not load this file, so a simpler player is used: {error}")],
    }
}

pub fn frame_image(path: &str, frame: f32, scale: f32) -> Option<Arc<Image>> {
    type Last = Option<(String, u32, u32, Arc<Image>)>;
    static LAST: Mutex<Last> = Mutex::new(None);
    let key = (frame.to_bits(), scale.to_bits());
    if let Some((name, a, b, image)) = LAST.lock().ok()?.as_ref() {
        if name == path && (*a, *b) == key {
            return Some(image.clone());
        }
    }
    let start = load(path).ok()?.start;
    let from_engine = voidedit_fx::lottie::engine(path).ok().and_then(|engine| engine.render(frame + start, scale)).map(|image| Image { width: image.width, height: image.height, pixels: image.pixels });
    let image = Arc::new(match from_engine {
        Some(image) => image,
        None => load(path).ok()?.render(frame, scale)?,
    });
    if let Ok(mut last) = LAST.lock() {
        *last = Some((path.to_string(), key.0, key.1, image.clone()));
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animation(text: &str) -> Animation {
        let folder = std::env::temp_dir().join(format!("voidedit-lottie-{}-{}", std::process::id(), text.len()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("a.json");
        std::fs::write(&path, text).unwrap();
        let parsed = parse(path.to_str().unwrap());
        let _ = std::fs::remove_dir_all(folder);
        parsed.unwrap()
    }

    fn at(image: &Image, x: usize, y: usize) -> [u8; 4] {
        let start = (y * image.width + x) * 4;
        [image.pixels[start], image.pixels[start + 1], image.pixels[start + 2], image.pixels[start + 3]]
    }

    const BOX: &str = r#"{"w":100,"h":100,"fr":30,"ip":0,"op":60,"layers":[{"ty":4,"ind":1,"ip":0,"op":60,"ks":{"p":{"k":[{"t":0,"s":[20,50],"o":{"x":[0.5],"y":[0.5]},"i":{"x":[0.5],"y":[0.5]}},{"t":30,"s":[80,50]}]},"a":{"k":[0,0]},"s":{"k":[100,100]},"r":{"k":0},"o":{"k":100}},"shapes":[{"ty":"gr","it":[{"ty":"sh","ks":{"k":{"v":[[-10,-10],[10,-10],[10,10],[-10,10]],"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"c":true}}},{"ty":"fl","c":{"k":[1,0,0,1]},"o":{"k":100}},{"ty":"tr","p":{"k":[0,0]},"a":{"k":[0,0]},"s":{"k":[100,100]},"r":{"k":0},"o":{"k":100}}]}]}]}"#;

    #[test]
    fn custom_paths_animate_along_their_keyframes() {
        let lottie = animation(BOX);
        let start = lottie.render(0.0, 1.0).unwrap();
        assert_eq!(at(&start, 20, 50), [255, 0, 0, 255]);
        assert_eq!(at(&start, 80, 50)[3], 0);
        let end = lottie.render(30.0, 1.0).unwrap();
        assert_eq!(at(&end, 80, 50), [255, 0, 0, 255]);
        assert_eq!(at(&end, 20, 50)[3], 0);
    }

    #[test]
    fn precomps_masks_and_mattes_all_render() {
        let source = r##"{"w":100,"h":100,"fr":30,"ip":0,"op":60,
            "assets":[{"id":"c","layers":[{"ty":1,"ip":0,"op":60,"sw":100,"sh":100,"sc":"#00ff00","ks":{}}]}],
            "layers":[
              {"ty":0,"refId":"c","ip":0,"op":60,"ks":{},"hasMask":true,"masksProperties":[{"mode":"a","pt":{"k":{"v":[[0,0],[50,0],[50,100],[0,100]],"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"c":true}},"o":{"k":100}}]}
            ]}"##;
        let image = animation(source).render(0.0, 1.0).unwrap();
        assert_eq!(at(&image, 25, 50), [0, 255, 0, 255]);
        assert_eq!(at(&image, 75, 50)[3], 0);
    }

    #[test]
    fn trim_paths_hide_part_of_a_stroke() {
        let source = r#"{"w":100,"h":100,"fr":30,"ip":0,"op":60,"layers":[{"ty":4,"ip":0,"op":60,"ks":{},"shapes":[{"ty":"sh","ks":{"k":{"v":[[0,50],[100,50]],"i":[[0,0],[0,0]],"o":[[0,0],[0,0]],"c":false}}},{"ty":"tm","s":{"k":0},"e":{"k":50},"o":{"k":0}},{"ty":"st","c":{"k":[1,1,1,1]},"o":{"k":100},"w":{"k":10}}]}]}"#;
        let image = animation(source).render(0.0, 1.0).unwrap();
        assert!(at(&image, 25, 50)[3] > 200);
        assert_eq!(at(&image, 75, 50)[3], 0);
    }

    #[test]
    fn invalid_files_report_an_error() {
        let folder = std::env::temp_dir().join(format!("voidedit-lottie-bad-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("bad.json");
        std::fs::write(&path, "{}").unwrap();
        assert!(parse(path.to_str().unwrap()).is_err());
        let _ = std::fs::remove_dir_all(folder);
    }
}
