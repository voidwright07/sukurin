//! Embeds the app icon as resource 1, which GPUI loads for every window and
//! Explorer shows for the executable.

fn main() {
    let icon = "../../assets/icon/sukurin.ico";
    println!("cargo:rerun-if-changed={icon}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon).set("ProductName", "Sukurin").set("FileDescription", "Sukurin");
        res.compile().expect("embed the app icon");
    }
}
