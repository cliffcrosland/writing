use std::str::FromStr;

use clap::{App, Arg};
use lazy_static::lazy_static;

lazy_static! {
    static ref CONFIG: Config = parse_command_line_flags();
}

#[derive(Debug, Default)]
pub struct Config {
    pub dynamodb_region: rusoto_core::Region,
    pub postgres_db_host: String,
    pub postgres_db_port: u32,
    pub postgres_db_password: String,
    pub postgres_db_pool_max_size: u32,
    pub http_port: u32,
    pub cookie_secret: String,
    pub cookie_secure: bool,
}

pub fn config() -> &'static Config {
    &CONFIG
}

fn parse_command_line_flags() -> Config {
    let matches = App::new("backend")
        .version("0.1")
        .arg(
            Arg::with_name("dynamodb_region")
                .help("The AWS region for DynamoDB. Default value is \"local\", for dev/testing.
                       Example value for staging/production: \"us-west-2\".")
                .takes_value(true)
                .value_name("AWS_REGION")
                .default_value("local")
        )
        .arg(
            Arg::with_name("postgres_db_host")
                .help("The host of the Postgres database")
                .takes_value(true)
                .value_name("HOST")
                .default_value("localhost"),
        )
        .arg(
            Arg::with_name("postgres_db_port")
                .help("The port of the Postgres database")
                .takes_value(true)
                .value_name("PORT")
                .default_value("5432"),
        )
        .arg(
            Arg::with_name("postgres_db_pool_max_size")
                .help("The max number of connections per Postgres pool")
                .takes_value(true)
                .value_name("MAX_SIZE")
                .default_value("128"),
        )
        .arg(
            Arg::with_name("http_port")
                .help("The port for the HTTP server")
                .takes_value(true)
                .value_name("PORT")
                .default_value("8080"),
        )
        .get_matches();

    // TODO(cliff): Choose the password environment variable in a better way.
    let pg_password_env_var = "WRITING_PG_DEV_PASSWORD";

    Config {
        dynamodb_region: match matches.value_of("dynamodb_region").unwrap() {
            "local" => rusoto_core::Region::Custom {
                name: "local".to_string(),
                endpoint: "http://localhost:8000".to_string(),
            },
            region_str => rusoto_core::Region::from_str(region_str).unwrap(),
        },
        postgres_db_host: matches.value_of("postgres_db_host").unwrap().to_string(),
        postgres_db_port: matches
            .value_of("postgres_db_port")
            .unwrap()
            .parse::<u32>()
            .unwrap(),
        postgres_db_pool_max_size: matches
            .value_of("postgres_db_pool_max_size")
            .unwrap()
            .parse::<u32>()
            .unwrap(),
        postgres_db_password: std::env::var(pg_password_env_var).unwrap_or_else(|_| {
            panic!(format!(
                "Could not find environment variable {}",
                &pg_password_env_var
            ));
        }),
        http_port: matches
            .value_of("http_port")
            .unwrap()
            .parse::<u32>()
            .unwrap(),
        cookie_secret: std::env::var("COOKIE_SECRET").unwrap_or_else(|_| {
            panic!("Could not find COOKIE_SECRET environment variable");
        }),
        cookie_secure: std::env::var("COOKIE_SECURE")
            .unwrap_or_else(|_| "true".to_string())
            .parse::<bool>()
            .unwrap(),
    }
}
