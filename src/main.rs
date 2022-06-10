#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

use actix_web::{get, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};
use database::migrate_database;
use uuid::Uuid;

use crate::{
    database::PgPool,
    error::Error,
    models::{Dataset, Dimension},
    score::SaveRequest,
};

mod database;
mod error;
mod models;
mod schema;
mod score;

#[get("/ping")]
async fn ping(pool: web::Data<PgPool>) -> Result<impl Responder, Error> {
    let conn = pool.get()?;
    conn.test_connection()?;
    Ok("pong")
}

#[get("/ready")]
async fn ready() -> Result<impl Responder, Error> {
    Ok("ok")
}

#[get("/api/v1/graph/{id}")]
async fn get_score_graph(
    id: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    let uuid = parse_uuid(id.into_inner())?;
    let mut conn = pool.get()?;

    let graph = conn
        .get_score_graph_by_id(uuid)?
        .ok_or(Error::NotFound(uuid))?;

    Ok(HttpResponse::Ok()
        .content_type("text/turtle")
        .message_body(graph))
}

#[get("/api/v1/score/{id}")]
async fn get_score_json(
    id: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    let uuid = parse_uuid(id.into_inner())?;
    let mut conn = pool.get()?;

    let score = conn
        .get_score_json_by_id(uuid)?
        .ok_or(Error::NotFound(uuid))?;

    Ok(HttpResponse::Ok()
        .content_type(mime::APPLICATION_JSON)
        .message_body(score))
}

#[post("/api/v1/save/{id}")]
async fn save(
    id: web::Path<String>,
    body: web::Json<SaveRequest>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    let uuid = parse_uuid(id.into_inner())?;
    let mut conn = pool.get()?;

    let graph = Dataset {
        id: uuid.to_string(),
        publisher_id: body.publisher_id.clone(),
        title: body.title.clone(),
        score_graph: body.graph.clone(),
        score_json: serde_json::to_string(&body.scores)?,
    };

    // TODO: use web::block(move || {}) for db operations

    conn.store_dataset(graph)?;
    conn.drop_dimensions(uuid)?;

    for dimension in &body.scores.dataset.dimensions {
        conn.store_dimension(Dimension {
            dataset_id: uuid.to_string(),
            title: dimension.name.clone(),
            score: dimension.score as i32,
            max_score: dimension.max_score as i32,
        })?;
    }

    Ok(HttpResponse::Accepted()
        .content_type(mime::APPLICATION_JSON)
        .message_body(""))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();

    migrate_database().unwrap();
    let pool = PgPool::new().unwrap();

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .service(ping)
            .service(ready)
            .service(get_score_graph)
            .service(get_score_json)
            .service(save)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

fn parse_uuid(uuid: String) -> Result<Uuid, Error> {
    Uuid::parse_str(uuid.as_ref()).map_err(|_| Error::InvalidID(uuid))
}
