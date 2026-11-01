use std::collections::HashMap;
use std::sync::Arc;

use eframe::wgpu;
use voidedit_core::{Blend, MaskShape, Project, Shape};
use voidedit_render::{layers, Image, Op, Resolved};

const LAYER_BYTES: usize = 11 * 16 + 48 * 16;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const VIEW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

struct Target {
    color: wgpu::Texture,
    backdrop: wgpu::Texture,
    backdrop_view: wgpu::TextureView,
    width: u32,
    height: u32,
}

pub struct GpuCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    target: Option<Target>,
    blank: wgpu::TextureView,
    cache: HashMap<usize, (Arc<Image>, wgpu::TextureView)>,
}

fn encode_ops(ops: &[Op]) -> (Vec<[f32; 4]>, bool) {
    let mut out = Vec::new();
    for op in ops {
        match *op {
            Op::Gain(value) => out.push([0.0, value, 0.0, 0.0]),
            Op::Mix(color, amount) => {
                out.push([1.0, color[0], color[1], color[2]]);
                out.push([amount, 0.0, 0.0, 0.0]);
            }
            Op::Invert => out.push([2.0, 0.0, 0.0, 0.0]),
            Op::Gray => out.push([3.0, 0.0, 0.0, 0.0]),
            Op::Sepia => out.push([4.0, 0.0, 0.0, 0.0]),
            Op::Contrast(value) => out.push([5.0, value, 0.0, 0.0]),
            Op::Saturation(value) => out.push([6.0, value, 0.0, 0.0]),
            Op::Warm(value) => out.push([7.0, value, 0.0, 0.0]),
            Op::Posterize(value) => out.push([8.0, value, 0.0, 0.0]),
            Op::Hue(value) => out.push([9.0, value, 0.0, 0.0]),
            Op::Gamma(value) => out.push([10.0, value, 0.0, 0.0]),
            Op::Levels(black, white) => out.push([11.0, black, white, 0.0]),
            Op::Balance(shift) => out.push([12.0, shift[0], shift[1], shift[2]]),
            Op::Threshold(value) => out.push([13.0, value, 0.0, 0.0]),
            Op::Wheels(lift, gamma, gain) => {
                out.push([14.0, lift[0], lift[1], lift[2]]);
                out.push([gain[0], gain[1], gain[2], 0.0]);
                out.push([gamma[0], gamma[1], gamma[2], 0.0]);
            }
            Op::Curve(a, b, c) => out.push([15.0, a, b, c]),
        }
    }
    let fits = out.len() <= 48;
    out.truncate(48);
    (out, fits)
}

fn encode_layer(layer: &Resolved, width: f32, height: f32) -> Option<Vec<u8>> {
    let (ops, fits) = encode_ops(&layer.ops);
    if !fits || layer.lut.is_some() {
        return None;
    }
    let (encoded_shape, baked) = if layer.image.is_some() {
        (2.0, layer.color)
    } else {
        (if layer.shape == Shape::Ellipse { 1.0 } else { 0.0 }, layer.color)
    };
    let key = layer.key.unwrap_or(([0.0; 3], 0.0, 1.0));
    let blend_index = voidedit_core::BLENDS.iter().position(|mode| *mode == layer.blend).unwrap_or(0) as f32;
    let op_count = if layer.image.is_some() { ops.len() as f32 } else { 0.0 };
    let mask = &layer.mask;
    let mask_kind = match mask.shape {
        MaskShape::None => 0.0,
        MaskShape::Rectangle => 1.0,
        MaskShape::Ellipse => 2.0,
    };
    let rows: [[f32; 4]; 11] = [
        [layer.x, layer.y, layer.half_w, layer.half_h],
        [layer.sin, layer.cos, layer.opacity, 0.0],
        [baked[0], baked[1], baked[2], encoded_shape],
        [layer.vignette, layer.pixelate, layer.noise, 0.0],
        [layer.flip_x as u8 as f32, layer.flip_y as u8 as f32, blend_index, op_count],
        [key.0[0], key.0[1], key.0[2], key.1],
        [key.2, layer.key.is_some() as u8 as f32, 0.0, 0.0],
        [mask.x, mask.y, mask.width, mask.height],
        [mask_kind, mask.feather, mask.invert as u8 as f32, 0.0],
        [layer.wipe_x, layer.wipe_y, layer.wave.0, layer.wave.1],
        [width, height, 0.0, 0.0],
    ];
    let mut bytes = Vec::with_capacity(LAYER_BYTES);
    for row in rows {
        for value in row {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for index in 0..48 {
        for value in ops.get(index).copied().unwrap_or([0.0; 4]) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Some(bytes)
}

impl GpuCompositor {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("composite"), source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()) });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                texture_entry(1),
                texture_entry(2),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bind_layout], push_constant_ranges: &[] });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vertex"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::COLOR })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let blank = upload(&device, &queue, &Image { width: 1, height: 1, pixels: vec![0; 4] }).create_view(&Default::default());
        Self { device, queue, pipeline, bind_layout, target: None, blank, cache: HashMap::new() }
    }

    fn ensure_target(&mut self, width: u32, height: u32) -> bool {
        if self.target.as_ref().is_some_and(|target| target.width == width && target.height == height) {
            return false;
        }
        let make = |usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage,
                view_formats: &[VIEW_FORMAT],
            })
        };
        let color = make(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let backdrop = make(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let backdrop_view = backdrop.create_view(&Default::default());
        self.target = Some(Target { color, backdrop, backdrop_view, width, height });
        true
    }

    pub fn display_view(&self) -> Option<wgpu::TextureView> {
        self.target.as_ref().map(|target| target.color.create_view(&wgpu::TextureViewDescriptor { format: Some(VIEW_FORMAT), ..Default::default() }))
    }

    pub fn size(&self) -> (u32, u32) {
        self.target.as_ref().map_or((0, 0), |target| (target.width, target.height))
    }

    pub fn render(&mut self, project: &Project, frame: u32, scale: f32, images: &dyn Fn(&str) -> Option<Arc<Image>>) -> Option<bool> {
        let width = ((project.width as f32 * scale.clamp(0.05, 1.0)) as u32).max(1);
        let height = ((project.height as f32 * scale.clamp(0.05, 1.0)) as u32).max(1);
        let resolved = layers(project, frame, scale, images);
        let encoded: Option<Vec<Vec<u8>>> = resolved.iter().map(|layer| encode_layer(layer, width as f32, height as f32)).collect();
        let encoded = encoded?;
        let resized = self.ensure_target(width, height);
        let mut live = HashMap::new();
        for layer in &resolved {
            if let Some(image) = &layer.image {
                let key = Arc::as_ptr(image) as usize;
                let view = match self.cache.remove(&key) {
                    Some(entry) => entry,
                    None => (image.clone(), upload(&self.device, &self.queue, image).create_view(&Default::default())),
                };
                live.insert(key, view);
            }
        }
        self.cache = live;
        let target = self.target.as_ref()?;
        let color_view = target.color.create_view(&Default::default());
        let background = project.background.map(|channel| channel as f64 / 255.0);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: background[0], g: background[1], b: background[2], a: 1.0 }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let mut retained = Vec::with_capacity(encoded.len());
        for (layer, bytes) in resolved.iter().zip(&encoded) {
            if layer.blend != Blend::Normal {
                encoder.copy_texture_to_texture(target.color.as_image_copy(), target.backdrop.as_image_copy(), wgpu::Extent3d { width, height, depth_or_array_layers: 1 });
            }
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: LAYER_BYTES as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.queue.write_buffer(&buffer, 0, bytes);
            let image_view = layer.image.as_ref().and_then(|image| self.cache.get(&(Arc::as_ptr(image) as usize))).map_or(&self.blank, |entry| &entry.1);
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.bind_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(image_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&target.backdrop_view) },
                ],
            });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &color_view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
            retained.push(buffer);
        }
        self.queue.submit([encoder.finish()]);
        Some(resized)
    }

    pub fn read_back(&self) -> Option<Vec<u8>> {
        let target = self.target.as_ref()?;
        let row = (target.width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * target.height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
            wgpu::Extent3d { width: target.width, height: target.height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let mapped = buffer.slice(..).get_mapped_range();
        let mut out = Vec::with_capacity((target.width * target.height * 4) as usize);
        for line in mapped.chunks(row as usize) {
            out.extend_from_slice(&line[..(target.width * 4) as usize]);
        }
        Some(out)
    }
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, image: &Image) -> wgpu::Texture {
    let size = wgpu::Extent3d { width: image.width.max(1) as u32, height: image.height.max(1) as u32, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &image.pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.width * 4), rows_per_image: None },
        size,
    );
    texture
}
