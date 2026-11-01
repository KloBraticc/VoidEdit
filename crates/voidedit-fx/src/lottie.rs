use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use effectcraft_expr::Expressions;
use effectcraft_keyframe::Value as Val;
use effectcraft_project::{Footage, ItemId, LayerId, LayerSource, Project};
use effectcraft_raster::Image;
use effectcraft_render::{FootageSource, RenderOpts, Renderer};
use effectcraft_time::Tick;
use serde_json::Value;

#[derive(Clone, Copy)]
enum Kind {
    Scalar(f64),
    Color,
    Vec3,
}

struct Binding {
    comp: ItemId,
    layer: LayerId,
    path: String,
    data: Value,
    kind: Kind,
}

pub struct Engine {
    project: Mutex<Project>,
    comp: ItemId,
    bindings: Vec<Binding>,
    footage: Pictures,
    pub warnings: Vec<String>,
    fps: f64,
}

#[derive(Default)]
struct Pictures {
    loaded: Mutex<HashMap<String, Option<Arc<Image>>>>,
}

impl FootageSource for Pictures {
    fn frame(&self, _item: ItemId, footage: &Footage, _t: Tick) -> Option<Arc<Image>> {
        let mut loaded = self.loaded.lock().ok()?;
        loaded
            .entry(footage.path.clone())
            .or_insert_with(|| {
                let picture = image::open(&footage.path).ok()?.into_rgba8();
                Some(Arc::new(Image::from_rgba8(picture.width(), picture.height(), picture.as_raw())))
            })
            .clone()
    }
}

fn numbers(value: &Value) -> Vec<f64> {
    match value {
        Value::Number(number) => vec![number.as_f64().unwrap_or(0.0)],
        Value::Array(items) => items.iter().filter_map(Value::as_f64).collect(),
        _ => Vec::new(),
    }
}

fn ease(key: &Value, x: f64) -> f64 {
    let one = |value: Option<&Value>| value.map(numbers).and_then(|list| list.first().copied());
    let (Some(out), Some(into)) = (key.get("o"), key.get("i")) else { return x };
    let (Some(x1), Some(y1), Some(x2), Some(y2)) = (one(out.get("x")), one(out.get("y")), one(into.get("x")), one(into.get("y"))) else { return x };
    let at = |a: f64, b: f64, t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
    let (mut low, mut high) = (0.0, 1.0);
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

fn sample(property: &Value, time: f64) -> Vec<f64> {
    let Some(data) = property.get("k") else { return Vec::new() };
    let Some(keys) = data.as_array().filter(|items| items.first().is_some_and(|item| item.get("t").is_some())) else { return numbers(data) };
    let frame = |key: &Value| key.get("t").and_then(Value::as_f64).unwrap_or(0.0);
    let start_of = |key: &Value| key.get("s").map(numbers).unwrap_or_default();
    if time <= frame(&keys[0]) {
        return start_of(&keys[0]);
    }
    for pair in keys.windows(2) {
        if time < frame(&pair[1]) {
            let from = start_of(&pair[0]);
            if pair[0].get("h").and_then(Value::as_i64) == Some(1) {
                return from;
            }
            let to = pair[0].get("e").or_else(|| pair[1].get("s")).map(numbers).unwrap_or_else(|| from.clone());
            let amount = ease(&pair[0], (time - frame(&pair[0])) / (frame(&pair[1]) - frame(&pair[0])).max(0.001));
            return from.iter().enumerate().map(|(index, a)| a + (to.get(index).copied().unwrap_or(*a) - a) * amount).collect();
        }
    }
    let last = &keys[keys.len() - 1];
    if last.get("s").is_some() {
        start_of(last)
    } else if keys.len() >= 2 {
        keys[keys.len() - 2].get("e").map(numbers).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn style_properties(kind: i64) -> Option<(&'static str, Vec<(&'static str, &'static str, Kind)>)> {
    let percent = Kind::Scalar(1.0);
    Some(match kind {
        0 => ("stroke", vec![("c", "color", Kind::Color), ("s", "size", percent), ("o", "opacity", percent)]),
        1 => ("dropShadow", vec![("c", "color", Kind::Color), ("o", "opacity", percent), ("a", "angle", percent), ("s", "size", percent), ("d", "distance", percent), ("ch", "spread", percent), ("no", "noise", percent)]),
        2 => ("innerShadow", vec![("c", "color", Kind::Color), ("o", "opacity", percent), ("a", "angle", percent), ("s", "size", percent), ("d", "distance", percent), ("ch", "choke", percent), ("no", "noise", percent)]),
        3 => ("outerGlow", vec![("c", "color", Kind::Color), ("o", "opacity", percent), ("r", "range", percent), ("ch", "spread", percent), ("no", "noise", percent), ("j", "jitter", percent), ("s", "size", percent)]),
        4 => ("innerGlow", vec![("c", "color", Kind::Color), ("o", "opacity", percent), ("r", "range", percent), ("ch", "choke", percent), ("no", "noise", percent), ("j", "jitter", percent), ("s", "size", percent)]),
        5 => ("bevelEmboss", vec![("sr", "depth", percent), ("s", "size", percent), ("a", "angle", percent), ("sf", "soften", percent), ("lc", "highlightColor", Kind::Color), ("lo", "highlightOpacity", percent), ("sc", "shadowColor", Kind::Color), ("so", "shadowOpacity", percent)]),
        6 => ("satin", vec![("c", "color", Kind::Color), ("o", "opacity", percent), ("a", "angle", percent), ("d", "distance", percent), ("s", "size", percent)]),
        7 => ("colorOverlay", vec![("c", "color", Kind::Color), ("so", "opacity", percent)]),
        8 => ("gradientOverlay", vec![("o", "opacity", percent), ("a", "angle", percent), ("gs", "scale", percent)]),
        _ => return None,
    })
}

fn add_styles_and_cameras(project: &mut Project, comp_id: ItemId, layers: &[Value], assets: &HashMap<String, ItemId>, bindings: &mut Vec<Binding>, warnings: &mut Vec<String>, depth: u32) {
    if depth > 12 {
        return;
    }
    let size = project.comp(comp_id).map_or((1920, 1080), |comp| (comp.width, comp.height));
    let mut remaining: Vec<LayerId> = project.comp(comp_id).map_or_else(Vec::new, |comp| comp.layers.iter().map(|layer| layer.id).collect());
    for json in layers {
        let kind = json.get("ty").and_then(Value::as_i64).unwrap_or(-1);
        let name = json.get("nm").and_then(Value::as_str).unwrap_or("");
        if kind == 13 {
            let Some(comp) = project.comp(comp_id).cloned() else { continue };
            let mut layer = effectcraft_project::build::layer(project, &comp, name, LayerSource::Camera, (size.0, size.1), None);
            let id = layer.id;
            let zoom = json.get("pe").map(|pe| sample(pe, 0.0)).and_then(|list| list.first().copied());
            if let (Some(zoom), Some(property)) = (zoom, layer.props.prop_mut("cameraOptions/zoom")) {
                property.value = Val::Scalar(zoom);
            }
            if let Some(comp) = project.comp_mut(comp_id) {
                layer.switches.three_d = true;
                comp.layers.insert(0, layer);
            }
            let transform = json.get("ks");
            for (key, path, value_kind) in [("p", "transform/position", Kind::Vec3), ("a", "transform/poi", Kind::Vec3), ("or", "transform/orientation", Kind::Vec3), ("rx", "transform/rotationX", Kind::Scalar(1.0)), ("ry", "transform/rotationY", Kind::Scalar(1.0)), ("rz", "transform/rotation", Kind::Scalar(1.0))] {
                if let Some(data) = transform.and_then(|ks| ks.get(key)) {
                    bindings.push(Binding { comp: comp_id, layer: id, path: path.to_string(), data: data.clone(), kind: value_kind });
                }
            }
            if let Some(data) = json.get("pe") {
                bindings.push(Binding { comp: comp_id, layer: id, path: "cameraOptions/zoom".to_string(), data: data.clone(), kind: Kind::Scalar(1.0) });
            }
            continue;
        }
        let found = remaining.iter().position(|id| project.comp(comp_id).and_then(|comp| comp.layer(*id)).is_some_and(|layer| layer.name == name));
        let Some(id) = found.map(|index| remaining.remove(index)) else { continue };
        if let Some(styles) = json.get("sy").and_then(Value::as_array) {
            for style in styles {
                let Some((group, properties)) = style.get("ty").and_then(Value::as_i64).and_then(style_properties) else {
                    warnings.push(format!("{name}: unknown layer style"));
                    continue;
                };
                let added = {
                    let Some(comp) = project.comp(comp_id) else { continue };
                    let light = comp.global_light.clone();
                    let mut next = project.next_id;
                    let done = project.comp_mut(comp_id).and_then(|comp| comp.layers.iter_mut().find(|layer| layer.id == id)).and_then(|layer| {
                        let mut ids = effectcraft_project::build::Ids(&mut next);
                        effectcraft_project::styles::add_style(layer, &mut ids, group, &light, true)
                    });
                    project.next_id = next;
                    done.is_some()
                };
                if !added {
                    continue;
                }
                bindings.push(Binding { comp: comp_id, layer: id, path: format!("layerStyles/{group}/useGlobalLight"), data: serde_json::json!({"k": 0}), kind: Kind::Scalar(-1.0) });
                for (key, property, value_kind) in properties {
                    if let Some(data) = style.get(key) {
                        bindings.push(Binding { comp: comp_id, layer: id, path: format!("layerStyles/{group}/{property}"), data: data.clone(), kind: value_kind });
                    }
                }
            }
        }
        if kind == 0 {
            let inner_id = json.get("refId").and_then(Value::as_str).and_then(|reference| assets.get(reference)).copied();
            let _ = inner_id;
        }
    }
}

impl Engine {
    fn load(path: &str) -> Result<Engine, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let folder = Path::new(path).parent().map_or_else(String::new, |parent| parent.to_string_lossy().into_owned());
        let name = Path::new(path).file_stem().map_or_else(|| "Animation".to_string(), |stem| stem.to_string_lossy().into_owned());
        let images = std::env::temp_dir().join("voidedit-lottie-images");
        let _ = std::fs::create_dir_all(&images);
        let mut store = |image: &effectcraft_lottie::ImportedImage| {
            let file = images.join(format!("{}-{}.{}", std::process::id(), image.id.replace(['/', '\\', ':'], "_"), image.ext));
            std::fs::write(&file, &image.bytes).ok()?;
            Some(file.to_string_lossy().into_owned())
        };
        let mut project = Project::default();
        let result = effectcraft_lottie::import(&mut project, &bytes, &name, &folder, &mut store).map_err(|error| error.to_string())?;
        let mut warnings: Vec<String> = result.warnings.into_iter().filter(|warning| !warning.contains("layer styles") && !warning.contains("camera layers")).collect();
        let mut bindings = Vec::new();
        let document = if bytes.starts_with(b"PK") { None } else { serde_json::from_slice::<Value>(&bytes).ok() };
        if let Some(document) = &document {
            let root = document.get("layers").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
            add_styles_and_cameras(&mut project, result.comp, root, &HashMap::new(), &mut bindings, &mut warnings, 0);
            let has_nested_extras = document.get("assets").and_then(Value::as_array).is_some_and(|assets| assets.iter().any(|asset| asset.get("layers").and_then(Value::as_array).is_some_and(|layers| layers.iter().any(|layer| layer.get("sy").is_some() || layer.get("ty").and_then(Value::as_i64) == Some(13)))));
            if has_nested_extras {
                warnings.push("layer styles or cameras inside precomps".to_string());
            }
        } else if bytes.starts_with(b"PK") {
            warnings.push("layer styles and cameras in .lottie packages".to_string());
        }
        let fps = project.comp(result.comp).map_or(30.0, |comp| comp.frame_rate.as_f64());
        Ok(Engine { project: Mutex::new(project), comp: result.comp, bindings, footage: Pictures::default(), warnings, fps })
    }

    pub fn render(&self, frame: f32, scale: f32) -> Option<voidedit_core::RgbaImage> {
        let mut project = self.project.lock().ok()?;
        for binding in &self.bindings {
            let Some(layer) = project.comp_mut(binding.comp).and_then(|comp| comp.layers.iter_mut().find(|layer| layer.id == binding.layer)) else { continue };
            let Some(property) = layer.props.prop_mut(&binding.path) else { continue };
            let values = sample(&binding.data, frame as f64);
            property.value = match binding.kind {
                Kind::Scalar(factor) if factor < 0.0 => Val::Bool(false),
                Kind::Scalar(factor) => Val::Scalar(values.first().copied().unwrap_or(0.0) * factor),
                Kind::Color => Val::Color([values.first().copied().unwrap_or(0.0), values.get(1).copied().unwrap_or(0.0), values.get(2).copied().unwrap_or(0.0), 1.0]),
                Kind::Vec3 => Val::Vec3([values.first().copied().unwrap_or(0.0), values.get(1).copied().unwrap_or(0.0), values.get(2).copied().unwrap_or(0.0)]),
            };
        }
        let time = Tick::from_seconds_f64(frame as f64 / self.fps.max(1.0));
        let opts = RenderOpts { scale: scale as f64, ..RenderOpts::default() };
        let mut renderer = Renderer::new(&project, &self.footage, opts);
        renderer.expr = Some(&Expressions);
        let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| renderer.comp_frame(self.comp, time))).ok()?;
        Some(voidedit_core::RgbaImage { width: image.width as usize, height: image.height as usize, pixels: image.to_rgba8() })
    }
}

pub fn engine(path: &str) -> Result<Arc<Engine>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Result<Arc<Engine>, String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().map_err(|error| error.to_string())?;
    cache.entry(path.to_string()).or_insert_with(|| std::panic::catch_unwind(|| Engine::load(path)).unwrap_or_else(|_| Err("The animation engine could not load this file".to_string())).map(Arc::new)).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, text: &str) -> Arc<Engine> {
        let folder = std::env::temp_dir().join(format!("voidedit-engine-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        std::fs::write(&path, text).unwrap();
        engine(path.to_str().unwrap()).unwrap()
    }

    fn at(image: &voidedit_core::RgbaImage, x: usize, y: usize) -> [u8; 4] {
        let start = (y * image.width + x) * 4;
        [image.pixels[start], image.pixels[start + 1], image.pixels[start + 2], image.pixels[start + 3]]
    }

    fn solid(extra: &str) -> String {
        format!(r##"{{"w":100,"h":100,"fr":30,"ip":0,"op":60,"layers":[{{"ty":1,"nm":"S","ind":1,"ip":0,"op":60,"sw":20,"sh":20,"sc":"#ff0000","ks":{{"a":{{"k":[10,10,0]}},"p":{{"k":[50,50,0]}},"s":{{"k":[100,100,100]}},"r":{{"k":0}},"o":{{"k":100}}}}{extra}}}]}}"##)
    }

    #[test]
    fn renders_a_plain_layer() {
        let image = run("plain.json", &solid("")).render(0.0, 1.0).unwrap();
        assert!(at(&image, 50, 50)[0] > 200 && at(&image, 50, 50)[3] > 200);
        assert_eq!(at(&image, 5, 5)[3], 0);
    }

    #[test]
    fn runs_javascript_expressions() {
        let source = r##"{"w":100,"h":100,"fr":30,"ip":0,"op":60,"layers":[{"ty":1,"nm":"S","ind":1,"ip":0,"op":60,"sw":20,"sh":20,"sc":"#ff0000","ks":{"a":{"k":[10,10,0]},"p":{"k":[50,50,0],"x":"var $bm_rt; $bm_rt = [20 + time * 30, 50, 0];"},"s":{"k":[100,100,100]},"r":{"k":0},"o":{"k":100}}}]}"##;
        let lottie = run("expr.json", source);
        let start = lottie.render(0.0, 1.0).unwrap();
        assert!(at(&start, 20, 50)[3] > 200, "{:?}", at(&start, 20, 50));
        let later = lottie.render(40.0, 1.0).unwrap();
        assert!(at(&later, 60, 50)[3] > 200, "{:?}", at(&later, 60, 50));
        assert_eq!(at(&later, 20, 50)[3], 0);
    }

    #[test]
    fn applies_layer_effects() {
        let effect = r#","ef":[{"ty":29,"nm":"Gaussian Blur","mn":"ADBE Gaussian Blur 2","ef":[{"ty":0,"nm":"Blurriness","mn":"ADBE Gaussian Blur 2-0001","v":{"k":12}}]}]"#;
        let image = run("effect.json", &solid(effect)).render(0.0, 1.0).unwrap();
        let edge = at(&image, 36, 50)[3];
        assert!(edge > 0 && edge < 255, "{edge}");
    }

    #[test]
    fn draws_layer_styles() {
        let style = r#","sy":[{"ty":1,"nm":"Drop Shadow","c":{"k":[0,0,1,1]},"o":{"k":100},"a":{"k":90},"s":{"k":0},"d":{"k":15},"ch":{"k":0},"no":{"k":0}}]"#;
        let image = run("style.json", &solid(style)).render(0.0, 1.0).unwrap();
        let blue = image.pixels.chunks_exact(4).filter(|pixel| pixel[3] > 100 && pixel[2] > 150 && pixel[0] < 80).count();
        assert!(blue > 40, "{blue}");
    }

    #[test]
    fn three_d_layers_use_a_camera() {
        let source = r##"{"w":100,"h":100,"fr":30,"ip":0,"op":60,"layers":[
            {"ty":13,"nm":"Cam","ind":1,"ip":0,"op":60,"pe":{"k":100},"ks":{"p":{"k":[50,50,-50]},"a":{"k":[50,50,0]}}},
            {"ty":1,"ddd":1,"nm":"S","ind":2,"ip":0,"op":60,"sw":20,"sh":20,"sc":"#ff0000","ks":{"a":{"k":[10,10,0]},"p":{"k":[50,50,0]},"s":{"k":[100,100,100]},"r":{"k":0},"o":{"k":100}}}]}"##;
        let image = run("camera.json", source).render(0.0, 1.0).unwrap();
        let wide = at(&image, 35, 50)[3];
        assert!(wide > 200, "camera zoom should double the layer, got {wide}");
    }

    #[test]
    fn text_animators_change_letters() {
        let text = |animators: &str| format!(r##"{{"w":200,"h":100,"fr":30,"ip":0,"op":60,"fonts":{{"list":[]}},"layers":[{{"ty":5,"nm":"T","ind":1,"ip":0,"op":60,"ks":{{"p":{{"k":[20,60,0]}},"a":{{"k":[0,0,0]}},"s":{{"k":[100,100,100]}},"r":{{"k":0}},"o":{{"k":100}}}},"t":{{"d":{{"k":[{{"t":0,"s":{{"t":"HI","s":40,"f":"Arial","fc":[1,1,1],"j":0}}}}]}},"p":{{}},"m":{{"a":{{"k":[0,0]}}}},"a":[{animators}]}}}}]}}"##);
        let moved = r#"{"s":{"t":0,"s":{"k":0},"e":{"k":100},"o":{"k":0},"r":1,"b":1,"sh":1},"a":{"p":{"k":[0,-30,0]}}}"#;
        let lit = |image: &voidedit_core::RgbaImage| image.pixels.chunks_exact(4).enumerate().filter(|(_, pixel)| pixel[3] > 128).map(|(index, _)| index / image.width).sum::<usize>();
        let plain = run("text-a.json", &text("")).render(0.0, 1.0).unwrap();
        let shifted = run("text-b.json", &text(moved)).render(0.0, 1.0).unwrap();
        assert!(lit(&plain) > 0);
        assert_ne!(lit(&plain), lit(&shifted));
    }
}
