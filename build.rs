fn main() {
    let mut res = winres::WindowsResource::new();
    res.set("FileDescription", "A multi-functional toolkit");
    res.set("ProductName", "JJ-Toolkit");
    res.set("LegalCopyright", "JJayRex");
    res.set("FileVersion", "0.2.0.0");
    res.set("ProductVersion", "0.2.0.0");
    res.compile().unwrap();
}
