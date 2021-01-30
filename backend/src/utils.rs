use sqlx::types::chrono::NaiveDateTime;

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

pub fn date_time_to_micros(date_time: NaiveDateTime) -> i64 {
    date_time.timestamp_nanos() / 1000
}
