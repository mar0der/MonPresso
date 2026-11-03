fn main() {
    println!("cargo:rerun-if-changed=assets/monpresso.rc");
    println!("cargo:rerun-if-changed=assets/monpresso.manifest");
    println!("cargo:rerun-if-changed=assets/monpresso.ico");
    // The manifest is embedded through the resource script, so stop the linker from adding its own.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    embed_resource::compile("assets/monpresso.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
