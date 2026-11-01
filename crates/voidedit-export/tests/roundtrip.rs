use std::sync::Arc;
use voidedit_core::{Project, Shape};
use voidedit_export::{export, Progress};
use voidedit_media::Library;

#[test]
fn exports_a_playable_file() {
    let mut project = Project { width: 320, height: 180, ..Project::default() };
    let mut clip = project.new_clip(Shape::Ellipse, 0);
    clip.end = 30;
    project.clips.push(clip);
    let path = std::env::temp_dir().join(format!("voidedit-test-{}.mp4", std::process::id()));
    export(&project, &Library::default(), &path, &Arc::new(Progress::default())).unwrap();
    let library = Library::default();
    let details = library.details(&path.to_string_lossy()).expect("the export opens");
    let _ = std::fs::remove_file(&path);
    assert_eq!((details.width, details.height), (320, 180));
    assert!(details.seconds > 0.9);
}

#[test]
fn exports_a_given_project() {
    let (Ok(project), Ok(output)) = (std::env::var("VOIDEDIT_TEST_PROJECT"), std::env::var("VOIDEDIT_TEST_OUTPUT")) else { return };
    let project = Project::from_json(&std::fs::read_to_string(project).unwrap()).unwrap();
    let started = std::time::Instant::now();
    export(&project, &Library::default(), output.as_ref(), &Arc::new(Progress::default())).unwrap();
    println!("exported in {:?}", started.elapsed());
}
