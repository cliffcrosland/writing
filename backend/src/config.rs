use std::str::FromStr;

use clap::{App, Arg};
use lazy_static::lazy_static;

lazy_static! {
    static ref CONFIG: Config = parse_command_line_flags();
}

#[derive(Debug, Default)]
pub struct Config {
    pub dynamodb_region: rusoto_core::Region,
    pub dynamodb_env: String,
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
                .help(
                    "The AWS region for DynamoDB. Default value is \"local\", for dev/testing.
                       Example value for staging/production: \"us-west-2\".",
                )
                .takes_value(true)
                .value_name("DYNAMODB_REGION")
                .default_value("local"),
        )
        .arg(
            Arg::with_name("dynamodb_env")
                .help(
                    "The environment prefix to use for DynamoDB tables. Default value is \"local\", for dev/testing.
                       Example values for staging/production: \"staging\" and \"production\".",
                )
                .takes_value(true)
                .value_name("DYNAMODB_ENV")
                .default_value("local"),
        )
        .arg(
            Arg::with_name("http_port")
                .help("The port for the HTTP server")
                .takes_value(true)
                .value_name("PORT")
                .default_value("8080"),
        )
        .get_matches();

    Config {
        dynamodb_region: match matches.value_of("dynamodb_region").unwrap() {
            "local" => rusoto_core::Region::Custom {
                name: "local".to_string(),
                endpoint: "http://127.0.0.1:8000".to_string(),
            },
            region_str => rusoto_core::Region::from_str(region_str).unwrap(),
        },
        dynamodb_env: matches.value_of("dynamodb_env").unwrap().to_string(),
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
