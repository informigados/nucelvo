fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        .with_bundled_translations("../../translations");
    println!("cargo:rerun-if-changed=../../translations");
    slint_build::compile_with_config("ui/main.slint", config).expect("compile Nucelvo UI");
}
