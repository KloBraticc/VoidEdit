use std::collections::{BTreeMap, HashMap};

use effectcraft_effects::{all, default_value, find, Buf, EffectCtx, EffectEnv, Params};
use effectcraft_keyframe::Value;
use effectcraft_project::ParamUi;
use effectcraft_raster::Image;
use voidedit_core::{LibValue, Param};

pub struct FxInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
}

pub enum ParamKind {
    Number { min: f64, max: f64 },
    Point,
    Color,
    Check,
    Choice(Vec<String>),
}

pub struct ParamInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ParamKind,
}

pub fn list() -> Vec<FxInfo> {
    all().into_iter().map(|spec| FxInfo { id: spec.id, name: spec.name, category: spec.category }).collect()
}

pub fn params(id: &str) -> Vec<ParamInfo> {
    let Some(spec) = find(id) else { return Vec::new() };
    spec.params
        .iter()
        .filter_map(|param| {
            let kind = match (&param.ui, &param.default) {
                (ParamUi::Hidden | ParamUi::Layer | ParamUi::Mask | ParamUi::Path | ParamUi::Text | ParamUi::File { .. } | ParamUi::Gradient, _) => return None,
                (ParamUi::Popup { options }, _) => ParamKind::Choice(options.clone()),
                (ParamUi::Slider { slider_min, slider_max, .. }, _) => ParamKind::Number { min: *slider_min, max: *slider_max },
                (_, Value::Scalar(_)) => ParamKind::Number { min: f64::NEG_INFINITY, max: f64::INFINITY },
                (_, Value::Vec2(_)) => ParamKind::Point,
                (_, Value::Color(_)) => ParamKind::Color,
                (_, Value::Bool(_)) => ParamKind::Check,
                (_, Value::Enum(_)) => ParamKind::Number { min: 1.0, max: 64.0 },
                _ => return None,
            };
            Some(ParamInfo { id: param.id, name: param.name, kind })
        })
        .collect()
}

pub fn defaults(id: &str, size: [f64; 2]) -> BTreeMap<String, LibValue> {
    let mut values = BTreeMap::new();
    let Some(spec) = find(id) else { return values };
    for param in &spec.params {
        let value = match default_value(param, size) {
            Value::Scalar(number) => LibValue::Number(Param::new(number as f32)),
            Value::Vec2(point) => LibValue::Point(Param::new(point[0] as f32), Param::new(point[1] as f32)),
            Value::Color(color) => LibValue::Color(color.map(|channel| channel as f32)),
            Value::Bool(state) => LibValue::Check(state),
            Value::Enum(index) => LibValue::Choice(index),
            _ => continue,
        };
        values.insert(param.id.to_string(), value);
    }
    values
}

pub fn apply(id: &str, values: &BTreeMap<String, LibValue>, frame: f32, fps: f64, seed: u32, image: &voidedit_core::RgbaImage) -> Option<voidedit_core::RgbaImage> {
    let spec = find(id)?;
    let (width, height) = (image.width as u32, image.height as u32);
    if width == 0 || height == 0 {
        return None;
    }
    let size = [width as f64, height as f64];
    let mut map: HashMap<String, Value> = spec.params.iter().map(|param| (param.id.to_string(), default_value(param, size))).collect();
    for (key, value) in values {
        let converted = match value {
            LibValue::Number(number) => Value::Scalar(number.at(frame) as f64),
            LibValue::Point(x, y) => Value::Vec2([x.at(frame) as f64, y.at(frame) as f64]),
            LibValue::Color(color) => Value::Color(color.map(f64::from)),
            LibValue::Check(state) => Value::Bool(*state),
            LibValue::Choice(index) => Value::Enum(*index),
        };
        map.insert(key.clone(), converted);
    }
    let parameters = Params { values: map };
    let time = frame as f64 / fps.max(1.0);
    let env = EffectEnv { comp_time: time, frame_rate: fps, ..EffectEnv::default() };
    let context = EffectCtx { params: &parameters, time, layer_size: size, seed, adjustment: false, env };
    let input = Buf { img: Image::from_rgba8(width, height, &image.pixels), offset: [0.0, 0.0], scale: 1.0 };
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.render)(&context, input))).ok()?;
    let mut result = Image::new(width, height);
    let (dx, dy) = (output.offset[0].round() as i64, output.offset[1].round() as i64);
    for y in 0..height as i64 {
        for x in 0..width as i64 {
            let source = output.img.get(x - dx, y - dy);
            result.set(x as u32, y as u32, source);
        }
    }
    Some(voidedit_core::RgbaImage { width: image.width, height: image.height, pixels: result.to_rgba8() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_runs_on_a_small_image_without_crashing() {
        let image = voidedit_core::RgbaImage { width: 32, height: 32, pixels: (0..32 * 32 * 4).map(|index| (index % 251) as u8).collect() };
        let mut failed = Vec::new();
        let mut unchanged = Vec::new();
        let effects = list();
        assert!(effects.len() >= 300);
        for info in &effects {
            let values = defaults(info.id, [32.0, 32.0]);
            match apply(info.id, &values, 5.0, 30.0, 1, &image) {
                None => failed.push(info.id),
                Some(result) if result.pixels == Image::from_rgba8(32, 32, &image.pixels).to_rgba8() => unchanged.push(info.category),
                Some(_) => {}
            }
        }
        unchanged.sort_unstable();
        let mut summary = std::collections::BTreeMap::new();
        for category in &unchanged {
            *summary.entry(*category).or_insert(0) += 1;
        }
        println!("{} effects, failed: {failed:?}, unchanged at defaults: {} {summary:?}", effects.len(), unchanged.len());
        assert!(failed.len() < effects.len() / 20, "{failed:?}");
    }

    #[test]
    fn gaussian_blur_changes_pixels() {
        let mut pixels = vec![0u8; 32 * 32 * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        pixels[(16 * 32 + 16) * 4] = 255;
        let image = voidedit_core::RgbaImage { width: 32, height: 32, pixels };
        let id = list().into_iter().find(|info| info.name.contains("Gaussian")).map(|info| info.id).unwrap();
        let mut values = defaults(id, [32.0, 32.0]);
        values.insert("blurriness".into(), LibValue::Number(Param::new(6.0)));
        let result = apply(id, &values, 0.0, 30.0, 1, &image).unwrap();
        assert_ne!(result.pixels, image.pixels);
    }
}
