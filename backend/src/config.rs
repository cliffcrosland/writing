use clap::{App, Arg};
use lazy_static::lazy_static;

lazy_static! {
    static ref CONFIG: Config = parse_command_line_flags();
}

#[derive(Debug, Default)]
pub struct Config {
    pub postgres_db_host: String,
    pub postgres_db_port: u32,
    pub postgres_db_password: String,
    pub postgres_db_pool_max_size: u32,
    pub grpc_port: u32,
}

pub fn config() -> &'static Config {
    &CONFIG
}

fn parse_command_line_flags() -> Config {
    let matches = App::new("backend")
        .version("0.1")
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
            Arg::with_name("grpc_port")
                .help("The port for the GRPC server")
                .takes_value(true)
                .value_name("PORT")
                .default_value("10000"),
        )
        .get_matches();

    // TODO(cliff): Choose the password environment variable in a better way.
    let pg_password_env_var = "WRITING_PG_DEV_PASSWORD";

    Config {
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
                "Could not find environment var {}",
                &pg_password_env_var
            ));
        }),
        grpc_port: matches
            .value_of("grpc_port")
            .unwrap()
            .parse::<u32>()
            .unwrap(),
    }
}
