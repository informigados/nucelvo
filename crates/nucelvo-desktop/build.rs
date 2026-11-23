fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version_macros = [
            format!("NUCELVO_MAJOR={}", env!("CARGO_PKG_VERSION_MAJOR")),
            format!("NUCELVO_MINOR={}", env!("CARGO_PKG_VERSION_MINOR")),
            format!("NUCELVO_PATCH={}", env!("CARGO_PKG_VERSION_PATCH")),
            format!("NUCELVO_VERSION=\"{}\"", env!("CARGO_PKG_VERSION")),
            format!(
                "NUCELVO_ICON=\"{}/../../assets/brand/nucelvo-icon.ico\"",
                env!("CARGO_MANIFEST_DIR").replace('\\', "/")
            ),
        ];
        println!("cargo:rerun-if-changed=resources/windows.rc");
        println!("cargo:rerun-if-changed=../../assets/brand/nucelvo-icon.ico");
        embed_resource::compile_for("resources/windows.rc", ["nucelvo"], version_macros)
            .manifest_required()
            .expect("embed the Windows app icon and version information");
    }
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        .with_bundled_translations("../../translations");
    println!("cargo:rerun-if-changed=../../translations");
    slint_build::compile_with_config("ui/main.slint", config).expect("compile Nucelvo UI");
}
