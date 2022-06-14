#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

use std::env;

use actix_web::{
    get, middleware::Logger, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use database::migrate_database;
use lazy_static::lazy_static;
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

lazy_static! {
    static ref API_TOKEN: String = env::var("API_TOKEN").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "API_TOKEN not found");
        std::process::exit(1)
    });
}

fn validate_api_key(request: HttpRequest) -> Result<(), Error> {
    let token = request
        .headers()
        .get("X-API-KEY")
        .ok_or(Error::Unauthorized("X-API-KEY header missing".to_string()))?
        .to_str()
        .map_err(|_| Error::Unauthorized("invalid api key".to_string()))?;

    if token == API_TOKEN.clone() {
        Ok(())
    } else {
        Err(Error::Unauthorized("Incorrect api key".to_string()))
    }
}

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
    request: HttpRequest,
    id: web::Path<String>,
    body: web::Json<SaveRequest>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;

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

    // Fail if API_TOKEN missing
    let _ = API_TOKEN.clone();

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
