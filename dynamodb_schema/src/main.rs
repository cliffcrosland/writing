use rusoto_dynamodb::{DeleteTableInput, DynamoDb, DynamoDbClient};

use dynamodb_schema::TABLE_DEFINITIONS;

fn print_usage() {
    println!("");
    println!("Usage: dynamodb_schema <command>");
    println!("");
    println!("Valid values for <command>:");
    println!("\tcreate_local_tables");
    println!("\tdelete_local_tables");
    println!("\treset_local_tables");
}

fn local_table_name(table_name: &str) -> String {
    format!("local-{}", table_name)
}

fn create_dynamodb_client() -> DynamoDbClient {
    let request_dispatcher = rusoto_core::request::HttpClient::new().unwrap();
    let credentials_provider = rusoto_credential::DefaultCredentialsProvider::new().unwrap();
    let region = rusoto_core::Region::Custom {
        name: "local".to_string(),
        endpoint: "http://127.0.0.1:8000".to_string(),
    };
    DynamoDbClient::new_with(request_dispatcher, credentials_provider, region)
}

async fn create_local_tables() {
    println!("Creating local tables...");
    let dynamodb_client = create_dynamodb_client();
    for table_def in TABLE_DEFINITIONS.iter() {
        let table_name = local_table_name(&table_def.table_name);
        let mut table_def = table_def.clone();
        table_def.table_name = table_name.clone();
        println!("Creating table {}...", &table_def.table_name);
        let result = dynamodb_client.create_table(table_def).await;
        if let Err(e) = result {
            eprintln!("\tFailed to create table {}. Error: {}", &table_name, e);
        }
    }
    println!("Done creating local tables.");
}

async fn delete_local_tables() {
    println!("Deleting local tables...");
    let dynamodb_client = create_dynamodb_client();
    for table_def in TABLE_DEFINITIONS.iter() {
        let table_name = local_table_name(&table_def.table_name);
        println!("Deleting table {}...", &table_name);
        let result = dynamodb_client.delete_table(DeleteTableInput {
            table_name: table_name.clone(),
        })
        .await;
        if let Err(e) = result {
            eprintln!("\tFailed to delete table {}. Error: {}", &table_name, e);
        }
    }
    println!("Done deleting local tables.");
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        print_usage();
        std::process::exit(1);
    }
    match &args[1][..] {
        "create_local_tables" => {
            create_local_tables().await;
        },
        "delete_local_tables" => {
            delete_local_tables().await;
        },
        "reset_local_tables" => {
            delete_local_tables().await;
            create_local_tables().await;
        },
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }
}
