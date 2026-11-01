struct Layer {
    transform: vec4<f32>,
    rotation_opacity: vec4<f32>,
    color_shape: vec4<f32>,
    effects: vec4<f32>,
    flags: vec4<f32>,
    key: vec4<f32>,
    key_soft: vec4<f32>,
    mask_box: vec4<f32>,
    mask_flags: vec4<f32>,
    wipe_wave: vec4<f32>,
    canvas: vec4<f32>,
    across: vec4<f32>,
    down: vec4<f32>,
    normal: vec4<f32>,
    ops: array<vec4<f32>, 48>,
};

@group(0) @binding(0) var<uniform> layer: Layer;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var backdrop: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn apply_ops(start: vec3<f32>) -> vec3<f32> {
    var c = start;
    var i = 0;
    let count = i32(layer.flags.w);
    while i < count {
        let head = layer.ops[i];
        let kind = i32(head.x);
        i += 1;
        switch kind {
            case 0: { c = c * head.y; }
            case 1: { c = mix(c, head.yzw, layer.ops[i].x); i += 1; }
            case 2: { c = 255.0 - c; }
            case 3: { c = vec3<f32>(luma(c)); }
            case 4: {
                c = vec3<f32>(dot(c, vec3<f32>(0.393, 0.769, 0.189)), dot(c, vec3<f32>(0.349, 0.686, 0.168)), dot(c, vec3<f32>(0.272, 0.534, 0.131)));
            }
            case 5: { c = (c - 127.5) * head.y + 127.5; }
            case 6: { let g = luma(c); c = g + (c - g) * head.y; }
            case 7: { c = c + vec3<f32>(head.y, 0.0, -head.y); }
            case 8: { c = round(c / 255.0 * head.y) / head.y * 255.0; }
            case 9: {
                let s = sin(head.y);
                let k = cos(head.y);
                c = vec3<f32>(
                    dot(c, vec3<f32>(0.213 + k * 0.787 - s * 0.213, 0.715 - k * 0.715 - s * 0.715, 0.072 - k * 0.072 + s * 0.928)),
                    dot(c, vec3<f32>(0.213 - k * 0.213 + s * 0.143, 0.715 + k * 0.285 + s * 0.14, 0.072 - k * 0.072 - s * 0.283)),
                    dot(c, vec3<f32>(0.213 - k * 0.213 - s * 0.787, 0.715 - k * 0.715 + s * 0.715, 0.072 + k * 0.928 + s * 0.072)));
            }
            case 10: { c = pow(max(c, vec3<f32>(0.0)) / 255.0, vec3<f32>(head.y)) * 255.0; }
            case 11: { c = (c - head.y) / max(head.z - head.y, 1.0) * 255.0; }
            case 12: { c = c + head.yzw; }
            case 13: { c = vec3<f32>(select(0.0, 255.0, luma(c) >= head.y)); }
            case 14: {
                let gain = layer.ops[i].xyz;
                let gamma = layer.ops[i + 1].xyz;
                i += 2;
                c = pow(max(c / 255.0 * gain + head.yzw, vec3<f32>(0.0)), 1.0 / max(gamma, vec3<f32>(0.05))) * 255.0;
            }
            case 15: {
                let t = clamp(c / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
                c = c + ((1.0 - t) * (1.0 - t) * head.y + 2.0 * t * (1.0 - t) * head.z + t * t * head.w) * 1.27;
            }
            case 19: { c = vec3<f32>(c.b, c.r, c.g); }
            case 16: { c = select(c, 255.0 - c, c > vec3<f32>(head.y)); }
            case 17: {
                let g = luma(c);
                let spread = max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
                c = g + (c - g) * (1.0 + head.y * (1.0 - spread / 255.0));
            }
            case 18: {
                c = mix(head.yzw, layer.ops[i].xyz, clamp(luma(c) / 255.0, 0.0, 1.0));
                i += 1;
            }
            default: {}
        }
    }
    return c;
}

fn blend(mode: i32, base: vec3<f32>, top: vec3<f32>) -> vec3<f32> {
    let b = base / 255.0;
    let t = top / 255.0;
    var r = t;
    switch mode {
        case 1: { r = b * t; }
        case 2: { r = 1.0 - (1.0 - b) * (1.0 - t); }
        case 3: { r = select(1.0 - 2.0 * (1.0 - b) * (1.0 - t), 2.0 * b * t, b < vec3<f32>(0.5)); }
        case 4: { r = b + t; }
        case 5: { r = min(b, t); }
        case 6: { r = max(b, t); }
        case 7: { r = abs(b - t); }
        case 8: { r = (1.0 - 2.0 * t) * b * b + 2.0 * t * b; }
        default: {}
    }
    return r * 255.0;
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let half_size = layer.transform.zw;
    if half_size.x <= 0.0 || half_size.y <= 0.0 { discard; }
    var d = position.xy - layer.transform.xy;
    if layer.normal.w > 0.0 {
        let direction = vec3<f32>(d, layer.normal.w);
        let offset = vec3<f32>(0.0, 0.0, layer.across.w + layer.normal.w);
        let denominator = dot(layer.normal.xyz, direction);
        if abs(denominator) < 0.00001 { discard; }
        let t = dot(layer.normal.xyz, offset) / denominator;
        if t <= 0.0 { discard; }
        let hit = direction * t - offset;
        d = vec2<f32>(dot(layer.across.xyz, hit), dot(layer.down.xyz, hit));
    }
    let s = layer.rotation_opacity.x;
    let k = layer.rotation_opacity.y;
    var local = vec2<f32>(d.x * k - d.y * s, d.x * s + d.y * k);
    if layer.wipe_wave.z != 0.0 {
        local.x += layer.wipe_wave.z * sin(local.y / half_size.y * layer.wipe_wave.w);
    }
    let shape = i32(layer.color_shape.w);
    var uv = (local + half_size) / (half_size * 2.0);
    var color = layer.color_shape.xyz;
    var alpha = 1.0;
    if shape == 2 {
        if abs(local.x) >= half_size.x || abs(local.y) >= half_size.y { discard; }
        var sample_uv = uv;
        if layer.flags.x > 0.5 { sample_uv.x = 1.0 - sample_uv.x; }
        if layer.flags.y > 0.5 { sample_uv.y = 1.0 - sample_uv.y; }
        let size = vec2<f32>(textureDimensions(image));
        if layer.effects.y > 1.0 {
            let step_size = layer.effects.y / size;
            sample_uv = floor(sample_uv / step_size) * step_size;
        }
        let texel = clamp(vec2<i32>(sample_uv * size), vec2<i32>(0), vec2<i32>(size) - 1);
        let pixel = textureLoad(image, texel, 0) * 255.0;
        alpha = pixel.a / 255.0;
        if layer.key_soft.y > 0.5 {
            alpha *= clamp((distance(pixel.rgb, layer.key.xyz) - layer.key.w) / layer.key_soft.x, 0.0, 1.0);
        }
        color = apply_ops(pixel.rgb);
    } else if shape == 1 {
        let n = local / half_size;
        alpha = clamp((1.0 - length(n)) * min(half_size.x, half_size.y) + 0.5, 0.0, 1.0);
    } else {
        alpha = clamp(min(half_size.x - abs(local.x), half_size.y - abs(local.y)) + 0.5, 0.0, 1.0);
    }
    alpha *= layer.rotation_opacity.z;
    if uv.x > layer.wipe_wave.x || uv.y > layer.wipe_wave.y { discard; }
    let mask_kind = i32(layer.mask_flags.x);
    if mask_kind != 0 {
        let m = (uv - layer.mask_box.xy) / max(layer.mask_box.zw / 2.0, vec2<f32>(0.001));
        var dist = max(abs(m.x), abs(m.y));
        if mask_kind == 2 { dist = length(m); }
        let feather = max(layer.mask_flags.y, 0.001) / max(min(layer.mask_box.z, layer.mask_box.w) / 2.0, 0.001);
        var value = clamp((1.0 - dist) / feather + 0.5, 0.0, 1.0);
        if layer.mask_flags.z > 0.5 { value = 1.0 - value; }
        alpha *= value;
    }
    if alpha <= 0.0 { discard; }
    if layer.effects.x > 0.0 {
        let n = local / half_size;
        color *= 1.0 - layer.effects.x * min(dot(n, n) / 2.0, 1.0);
    }
    if layer.effects.z > 0.0 {
        let grain = fract(sin(dot(floor(position.xy), vec2<f32>(12.9898, 78.233))) * 43758.5453) - 0.5;
        color += grain * layer.effects.z * 2.55;
    }
    let mode = i32(layer.flags.z);
    if mode != 0 {
        let base = textureLoad(backdrop, vec2<i32>(position.xy), 0).rgb * 255.0;
        color = blend(mode, base, color);
    }
    return vec4<f32>(clamp(color / 255.0, vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
}
