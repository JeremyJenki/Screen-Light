fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("src/resources/icon.ico");
    res.set("FileDescription", "Screen Light");
    res.set("ProductName", "Screen Light");
    res.compile().unwrap();
}
