fn main() -> anyhow::Result<()> {
    tonic_build::configure()
        .build_client(false)
        .build_server(true)
        .compile(&["../proto/page.proto"], &["../proto"])?;
    Ok(())
}
