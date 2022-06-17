#[macro_use]
extern crate serde;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

use std::{env, str::from_utf8};

use actix_cors::Cors;
use actix_web::{
    get, http, middleware::Logger, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use database::migrate_database;
use lazy_static::lazy_static;
use utoipa::openapi::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use crate::{
    database::PgPool,
    db_models::{Dataset, Dimension},
    error::Error,
};

mod database;
mod db_models;
mod error;
mod models;
mod schema;

lazy_static! {
    static ref API_KEY: String = env::var("API_KEY").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "API_KEY not found");
        std::process::exit(1)
    });
    static ref ENVIRONMENT: String = env::var("ENVIRONMENT").unwrap_or_else(|e| {
        tracing::error!(error = e.to_string().as_str(), "ENVIRONMENT not found");
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

    if token == API_KEY.clone() {
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

#[post("/api/scores")]
async fn scores(pool: web::Data<PgPool>, body: web::Bytes) -> Result<impl Responder, Error> {
    let ids = serde_json::from_str::<Vec<String>>(from_utf8(&body)?)?
        .into_iter()
        .map(|id| parse_uuid(id))
        .collect::<Result<Vec<Uuid>, Error>>()?;

    let mut conn = pool.get()?;

    let response = models::DatasetsScores {
        scores: conn.json_scores(&ids)?,
        aggregations: conn.dimension_aggregates(&ids)?,
    };

    Ok(HttpResponse::Ok()
        .content_type(mime::APPLICATION_JSON)
        .message_body(serde_json::to_string(&response)?))
}

#[get("/api/scores/{id}")]
async fn json_score(
    id: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    let uuid = parse_uuid(id.into_inner())?;
    let mut conn = pool.get()?;

    let score = conn.json_score(uuid)?.ok_or(Error::NotFound(uuid))?;

    Ok(HttpResponse::Ok()
        .content_type(mime::APPLICATION_JSON)
        .message_body(score))
}

#[get("/api/graphs/{id}")]
async fn graph_score(
    id: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    let uuid = parse_uuid(id.into_inner())?;
    let mut conn = pool.get()?;

    let graph = conn.graph_score(uuid)?.ok_or(Error::NotFound(uuid))?;

    Ok(HttpResponse::Ok()
        .content_type("text/turtle")
        .message_body(graph))
}

#[post("/api/scores/{id}/update")]
async fn update_score(
    request: HttpRequest,
    body: web::Bytes,
    id: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;
    let update: models::ScorePutRequest = serde_json::from_str(from_utf8(&body)?)?;

    let uuid = parse_uuid(id.into_inner())?;
    let mut conn = pool.get()?;

    let graph = Dataset {
        id: uuid.to_string(),
        score_graph: update.graph.clone(),
        score_json: serde_json::to_string(&update.scores)?,
    };

    // TODO: use web::block(move || {}) for db operations

    conn.store_dataset(graph)?;
    conn.drop_dimensions(uuid)?;

    for dimension in &update.scores.dataset.dimensions {
        conn.store_dimension(Dimension {
            dataset_id: uuid.to_string(),
            id: dimension.id.clone(),
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

    // Fail if API_KEY missing
    let _ = API_KEY.clone();

    let openapi = serde_yaml::from_str::<OpenApi>(include_str!("../openapi.yaml")).unwrap();

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin_fn(|origin, _req_head| {
                if ENVIRONMENT.clone() == "staging" {
                    origin.as_bytes().ends_with(b"localhost:3001")
                        || origin
                            .as_bytes()
                            .ends_with(b"staging.fellesdatakatalog.digdir.no")
                        || origin.as_bytes().ends_with(b"34.117.84.181")
                } else if ENVIRONMENT.clone() == "demo" {
                    origin
                        .as_bytes()
                        .ends_with(b"demo.fellesdatakatalog.digdir.no")
                } else {
                    origin.as_bytes().ends_with(b"data.norge.no")
                        || origin.as_bytes().ends_with(b"datafabrikken.norge.no")
                }
            })
            .allowed_methods(vec!["GET", "POST"])
            .allowed_header("X-API-KEY")
            .allowed_header(http::header::ACCEPT)
            .max_age(3600);

        App::new()
            .wrap(cors)
            .wrap(Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .service(ping)
            .service(ready)
            .service(json_score)
            .service(graph_score)
            .service(scores)
            .service(update_score)
            .service(SwaggerUi::new("/swagger-ui/{_:.*}").url("/openapi.json", openapi.clone()))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

fn parse_uuid(uuid: String) -> Result<Uuid, Error> {
    Uuid::parse_str(uuid.as_ref()).map_err(|_| Error::InvalidID(uuid))
}
