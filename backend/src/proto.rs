pub mod writing {
    tonic::include_proto!("writing");
}

#[allow(dead_code)]
pub fn encode_protobuf_message<M>(message: &M) -> Result<Vec<u8>, prost::EncodeError>
where
    M: prost::Message,
{
    let mut encoded = Vec::new();
    match message.encode(&mut encoded) {
        Ok(_) => Ok(encoded),
        Err(e) => Err(e),
    }
}
