use voidedit_media::{FrameWant, Frames, Library};

#[test]
fn decodes_a_real_file_when_one_is_given() {
    let Ok(path) = std::env::var("VOIDEDIT_TEST_MEDIA") else { return };
    let library = Library::default();
    let details = library.details(&path).expect("the file opens");
    assert!(details.has_video && details.seconds > 0.5);
    let frames = Frames::new(library, Box::new(|| {}));
    frames.want(vec![FrameWant { path: path.clone(), seconds: 1.0, scale: 0.5 }]);
    let started = std::time::Instant::now();
    while frames.latest(&voidedit_core::media_key(&path, 1.0)).is_none() && started.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let image = frames.latest(&voidedit_core::media_key(&path, 1.0)).expect("a frame arrives");
    assert!(image.width > 1 && image.pixels.iter().any(|value| *value > 20));
    println!("decoded {}x{} in {:?}", image.width, image.height, started.elapsed());
}
