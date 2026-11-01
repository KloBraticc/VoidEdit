use eframe::wgpu;

fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).expect("adapter");
    let device = pollster::block_on(adapter.request_device(&Default::default(), None)).expect("device");
    println!("{}", adapter.get_info().backend.to_str());
    std::thread::sleep(std::time::Duration::from_secs(8));
    drop(device);
}
