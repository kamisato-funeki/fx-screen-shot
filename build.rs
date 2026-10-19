fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        match embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional() {
            Ok(_) => {}
            Err(err) => {
                println!("cargo:warning=resource embedding skipped: {err}");
            }
        }
    }
}
