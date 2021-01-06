fn main() -> anyhow::Result<()> {
    tonic_build::configure()
        .build_client(false)
        .build_server(true)
        .compile(
            &["../proto/backend_service.proto", "../proto/page.proto"],
            &["../proto"],
        )?;
    Ok(())
}
