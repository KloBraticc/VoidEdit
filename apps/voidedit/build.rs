fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/voidedit.ico").set("ProductName", "Void Edit").set("FileDescription", "Void Edit");
        resource.compile().expect("icon resource");
    }
}
