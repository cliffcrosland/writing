mod config;
mod db;
mod proto;

use sqlx::postgres::PgPool;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use config::config;
use proto::writing::backend_service_server::{BackendService, BackendServiceServer};
use proto::writing::{
    CreatePageRequest, CreatePageResponse, DeletePageNodeRequest, DeletePageNodeResponse,
    InsertPageNodeRequest, InsertPageNodeResponse, LoadPageRequest, LoadPageResponse,
    UpdatePageNodeRequest, UpdatePageNodeResponse, UpdatePageTitleRequest, UpdatePageTitleResponse,
};

struct BackendServiceImpl {
    db_pool: PgPool,
}

#[tonic::async_trait]
impl BackendService for BackendServiceImpl {
    async fn create_page(
        &self,
        _request: Request<CreatePageRequest>,
    ) -> Result<Response<CreatePageResponse>, Status> {
        unimplemented!();
    }

    async fn update_page_title(
        &self,
        _request: Request<UpdatePageTitleRequest>,
    ) -> Result<Response<UpdatePageTitleResponse>, Status> {
        unimplemented!();
    }

    async fn load_page(
        &self,
        _request: Request<LoadPageRequest>,
    ) -> Result<Response<LoadPageResponse>, Status> {
        unimplemented!();
    }

    async fn insert_page_node(
        &self,
        _request: Request<InsertPageNodeRequest>,
    ) -> Result<Response<InsertPageNodeResponse>, Status> {
        unimplemented!();
    }

    async fn update_page_node(
        &self,
        _request: Request<UpdatePageNodeRequest>,
    ) -> Result<Response<UpdatePageNodeResponse>, Status> {
        unimplemented!();
    }

    async fn delete_page_node(
        &self,
        _request: Request<DeletePageNodeRequest>,
    ) -> Result<Response<DeletePageNodeResponse>, Status> {
        unimplemented!();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let backend_service = BackendServiceImpl {
        db_pool: db::create_pool().await?,
    };

    dbg!(&backend_service.db_pool);

    log::info!(
        "Starting Backend GRPC server on port {}",
        config().grpc_port
    );
    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", config().grpc_port).parse()?;
    Server::builder()
        .add_service(BackendServiceServer::new(backend_service))
        .serve(addr)
        .await?;

    Ok(())
}
