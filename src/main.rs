use actix_web::{get, middleware::Logger, web, App, HttpServer, Responder};
use database::connection_pool;
use deadpool_postgres::{Client, Pool};
use uuid::Uuid;

use crate::{
    database::{get_graph_by_id, test_connection},
    error::Error,
    score::ScoreGraph,
};

mod database;
mod error;
mod score;
mod vocab;

#[get("/ping")]
async fn ping(pool: web::Data<Pool>) -> Result<impl Responder, Error> {
    let client: Client = pool.get().await?;
    test_connection(&client).await?;
    Ok("pong")
}

#[get("/ready")]
async fn ready() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/{id}")]
async fn get_score(id: web::Path<String>, pool: web::Data<Pool>) -> Result<impl Responder, Error> {
    let uuid = Uuid::parse_str(id.as_ref()).map_err(|_| Error::InvalidID(id.into_inner()))?;

    let client: Client = pool.get().await?;

    if let Some(graph_string) = get_graph_by_id(&client, uuid).await? {
        let score = ScoreGraph::parse(graph_string)?.score()?;
        Ok(web::Json(score))
    } else {
        Err(Error::NotFound(uuid))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();

    let pool = connection_pool().unwrap();

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .service(ping)
            .service(ready)
            .service(get_score)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
