//! Windows: puts the Freo icon and version information into freo.exe (the Explorer, the taskbar
//! and the installer's shortcuts show them). Needs the Windows SDK's resource compiler, so it only
//! runs when building on Windows.
fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/freo.ico");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if target_windows && cfg!(windows) {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/freo.ico");
        res.set("ProductName", "Freo");
        res.set("FileDescription", "Freo – parametrisches 3D-CAD");
        res.set("CompanyName", "Freo");
        res.set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon resource not embedded: {}", e);
        }
    }
}
