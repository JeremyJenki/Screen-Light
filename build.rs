fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let icon_light = format!("{}\\src\\resources\\icon.ico", manifest_dir);
    let icon_dark  = format!("{}\\src\\resources\\icon-dark.ico", manifest_dir);

    let mut res = winres::WindowsResource::new();
    // Resource ID 1: exe/main icon (light mode tray default)
    res.set_icon(&icon_light);
    // Resource ID 2: dark mode tray icon
    res.set_icon_with_id(&icon_dark, "2");
    res.set("FileDescription", "Screen Light");
    res.set("ProductName", "Screen Light");
    res.compile().unwrap();
}
