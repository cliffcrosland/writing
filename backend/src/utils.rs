use chrono::{DateTime, Utc};

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

pub fn get_date_time_millis_string(date_time: &DateTime<Utc>) -> String {
    date_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
