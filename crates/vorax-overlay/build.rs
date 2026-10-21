// The icon in the .exe (Explorer, the taskbar, the tray): assets/vorax.ico, drawn by tools/make-icon.py.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/vorax.rc");
        println!("cargo:rerun-if-changed=assets/vorax.ico");
        embed_resource::compile("assets/vorax.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
