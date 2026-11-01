use eframe::wgpu;
use voidedit_core::{Blend, Effect, Project, Shape};
use voidedit_gpu::GpuCompositor;
use voidedit_render::{render, Frame};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
    pollster::block_on(adapter.request_device(&Default::default(), None)).ok()
}

#[test]
fn gpu_matches_the_cpu_renderer() {
    let Some((device, queue)) = device() else { return };
    let mut project = Project { width: 64, height: 36, background: [20, 30, 40], ..Project::default() };
    let mut solid = project.new_clip(Shape::Solid, 0);
    solid.track = 1;
    solid.color = [200, 100, 50];
    solid.effects.push(Effect::Invert);
    let mut ball = project.new_clip(Shape::Ellipse, 0);
    ball.track = 2;
    ball.width = 30.0;
    ball.height = 30.0;
    ball.color = [60, 200, 120];
    ball.blend = Blend::Multiply;
    ball.rotation_y = voidedit_core::Param::new(35.0);
    ball.depth = voidedit_core::Param::new(-200.0);
    project.clips = vec![solid, ball];
    let mut cpu = Frame::default();
    render(&project, 0, 1.0, &mut cpu, &|_| None);
    let mut gpu = GpuCompositor::new(device, queue);
    gpu.render(&project, 0, 1.0, &|_| None).unwrap();
    let pixels = gpu.read_back().unwrap();
    assert_eq!(pixels.len(), cpu.pixels.len());
    let worst = pixels.iter().zip(&cpu.pixels).enumerate().filter(|(index, _)| index % 4 != 3).map(|(_, (a, b))| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(worst <= 3, "largest channel difference {worst}");
}
