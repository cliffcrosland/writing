fn main() -> anyhow::Result<()> {
    tonic_build::configure()
        .build_client(false)
        .build_server(false)
        .compile(&["../proto/document.proto"], &["../proto"])?;
    Ok(())
}
