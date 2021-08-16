fn main() -> Result<(), std::io::Error> {
    tonic_build::configure()
        .build_client(false)
        .build_server(false)
        .type_attribute("writing.CreateDocumentResponse", "#[derive(serde::Serialize)]")
        .type_attribute("writing.GetDocumentResponse", "#[derive(serde::Serialize)]")
        .type_attribute("writing.Document", "#[derive(serde::Serialize)]")
        .type_attribute("writing.ListMyDocumentsResponse", "#[derive(serde::Serialize)]")
        .compile(&["../proto/document.proto"], &["../proto"])?;
    Ok(())
}
