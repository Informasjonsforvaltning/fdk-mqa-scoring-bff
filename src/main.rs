#[macro_use]
extern crate serde;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

use std::{env, str::from_utf8};

use ::http::{uri::InvalidUri, Uri};
use actix_cors::Cors;
use actix_web::{
    get,
    http::{self, header},
    middleware::Logger,
    post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use database::migrate_database;
use lazy_static::lazy_static;
use utoipa::openapi::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use crate::{
    database::PgPool,
    db_models::{DatasetAssessment, Dimension},
    error::Error,
    models::DatasetsRequest,
};

mod database;
mod db_models;
mod error;
#[allow(dead_code, non_snake_case)]
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

#[get("/api/assessments/{id}")]
async fn assessment_graph(
    accept: web::Header<header::Accept>,
    id: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    let uuid = parse_uuid(id.into_inner())?;
    let mut conn = pool.get()?;

    if accept
        .0
        .iter()
        .any(|qi| qi.item.to_string() == "application/ld+json")
    {
        let graph = conn.jsonld_assessment(uuid)?.ok_or(Error::NotFound(uuid))?;

        Ok(HttpResponse::Ok()
            .content_type("application/ld+json")
            .message_body(graph))
    } else {
        let graph = conn.turtle_assessment(uuid)?.ok_or(Error::NotFound(uuid))?;

        Ok(HttpResponse::Ok()
            .content_type("text/turtle")
            .message_body(graph))
    }
}

#[post("/api/assessments/{id}")]
async fn update_assessment(
    request: HttpRequest,
    body: web::Bytes,
    id: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<impl Responder, Error> {
    validate_api_key(request)?;
    let uuid = parse_uuid(id.into_inner())?;
    let update: models::ScorePostRequest = serde_json::from_str(from_utf8(&body)?)?;
    let dataset_uri = update.scores.as_ref().dataset.id.clone();

    let mut conn = pool.get()?;

    let assessment = DatasetAssessment {
        id: uuid.to_string(),
        dataset_uri: dataset_uri.clone(),
        turtle_assessment: update.turtle_assessment.clone(),
        jsonld_assessment: update.jsonld_assessment.clone(),
        json_score: serde_json::to_string(&update.scores)?,
    };

    // TODO: use web::block(move || {}) for db operations

    conn.drop_dataset_dimensions(&dataset_uri)?;
    conn.store_dataset(assessment)?;

    for dimension in &update.scores.dataset.dimensions {
        conn.store_dimension(Dimension {
            dataset_uri: dataset_uri.clone(),
            id: dimension.id.clone(),
            score: dimension.score as i32,
            max_score: dimension.max_score as i32,
        })?;
    }

    Ok(HttpResponse::Accepted()
        .content_type(mime::APPLICATION_JSON)
        .message_body(""))
}

#[post("/api/scores")]
async fn scores(pool: web::Data<PgPool>, body: web::Bytes) -> Result<impl Responder, Error> {
    let dataset_uris = serde_json::from_str::<DatasetsRequest>(from_utf8(&body)?)?
        .datasets
        .into_iter()
        .map(|uri| uri.parse::<Uri>())
        .collect::<Result<Vec<Uri>, InvalidUri>>()?;
    let mut conn = pool.get()?;

    let response = models::DatasetsScores {
        scores: conn.json_scores(&dataset_uris)?,
        aggregations: conn.dimension_aggregates(&dataset_uris)?,
    };

    Ok(HttpResponse::Ok()
        .content_type(mime::APPLICATION_JSON)
        .message_body(serde_json::to_string(&response)?))
}

#[post("/api/assessments")]
async fn assessments(
    accept: web::Header<header::Accept>,
    pool: web::Data<PgPool>,
    body: web::Bytes,
) -> Result<impl Responder, Error> {
    let _ids = serde_json::from_str::<DatasetsRequest>(from_utf8(&body)?)?.datasets;
    let mut _conn = pool.get()?;

    if accept
        .0
        .iter()
        .any(|qi| qi.item.to_string() == "application/ld+json")
    {
        // TODO: fetch graphs in jsonld format
        let graphs = "";

        Ok(HttpResponse::Ok()
            .content_type("application/ld+json")
            .message_body(graphs))
    } else {
        // TODO: fetch graphs in turtle format
        let graphs = "";

        Ok(HttpResponse::Ok()
            .content_type("text/turtle")
            .message_body(graphs))
    }
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
            .allow_any_method()
            .allow_any_header()
            .allow_any_origin()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .wrap(Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .service(ping)
            .service(ready)
            .service(assessment_graph)
            .service(update_assessment)
            .service(assessments)
            .service(scores)
            .service(SwaggerUi::new("/swagger-ui/{_:.*}").url("/openapi.json", openapi.clone()))
    })
    .bind(("0.0.0.0", 8082))?
    .run()
    .await
}

fn parse_uuid(uuid: String) -> Result<Uuid, Error> {
    Uuid::parse_str(uuid.as_ref()).map_err(|_| Error::InvalidID(uuid))
}
