fn main() {
    println!("cargo:rerun-if-changed=assets/icons/sasha.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }

    // GPUI loads the window and taskbar icon from resource id 1.
    winresource::WindowsResource::new()
        .set_icon("assets/icons/sasha.ico")
        .compile()
        .expect("embed the Sasha application icon");
}
