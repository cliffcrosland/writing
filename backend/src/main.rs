mod config;
mod db;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let pool = db::create_pool().await?;

    dbg!(&pool);

    Ok(())
}
