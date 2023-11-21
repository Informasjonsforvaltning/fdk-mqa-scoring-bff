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
    body::{BoxBody, EitherBody},
    dev::{ServiceFactory, ServiceRequest, ServiceResponse},
    get,
    http::header,
    middleware::Logger,
    post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use database::migrate_database;
use lazy_static::lazy_static;
use utoipa::openapi::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use uuid::Uuid;

use crate::{
    database::{PgPool, DatabaseError},
    db_models::{DatasetAssessment, Dimension},
    error::Error,
    models::{DatasetsRequest, DatasetsScores},
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

    let result = web::block(move || {
        // Obtaining a connection from the pool is also a potentially blocking operation.
        // So, it should be called within the `web::block` closure, as well.
        let mut conn = pool.get()?;
        conn.test_connection()
    })
    .await
    .map_err(|e| {
        Error::BlockingError(e.into())
    })?;

    match result {
        Ok(_) => Ok("pong"),
        Err(e) => Err(e.into()),
    }
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
    let accept_json_ld = accept
        .0
        .iter()
        .any(|qi| qi.item.to_string() == "application/ld+json");

    let result = web::block(move || {
        // Obtaining a connection from the pool is also a potentially blocking operation.
        // So, it should be called within the `web::block` closure, as well.
        let mut conn = pool.get()?;
        if accept_json_ld
        {
            conn.jsonld_assessment(uuid)?.ok_or(Error::NotFound(uuid))
            
        } else {
            conn.turtle_assessment(uuid)?.ok_or(Error::NotFound(uuid))
        }
    })
    .await
    .map_err(|e| {
        Error::BlockingError(e.into())
    })?;
    
    match result {
        Ok(graph) => Ok(HttpResponse::Ok()
            .content_type(if accept_json_ld { "application/ld+json" } else { "text/turtle" })
            .message_body(graph)),
        Err(e) => Err(e.into()),
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

    let result: Result<(), DatabaseError> = web::block(move || {
        // Obtaining a connection from the pool is also a potentially blocking operation.
        // So, it should be called within the `web::block` closure, as well.
        let mut conn = pool.get()?;

        let assessment = DatasetAssessment {
            id: uuid.to_string(),
            dataset_uri: dataset_uri.clone(),
            turtle_assessment: update.turtle_assessment.clone(),
            jsonld_assessment: update.jsonld_assessment.clone(),
            json_score: serde_json::to_string(&update.scores)?,
        };

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

        Ok(())
    })
    .await
    .map_err(|e| {
        Error::BlockingError(e.into())
    })?;

    match result {
        Ok(_) => Ok(HttpResponse::Accepted()
            .content_type(mime::APPLICATION_JSON)
            .message_body("")),
        Err(e) => Err(e.into()),
    }    
}

#[post("/api/scores")]
async fn scores(pool: web::Data<PgPool>, body: web::Bytes) -> Result<impl Responder, Error> {
    let data = serde_json::from_str::<DatasetsRequest>(from_utf8(&body)?)?;
    // Check that uris are valid, but disregard parsed value.
    let _parsed_dataset_uris = data
        .datasets
        .iter()
        .map(|uri| uri.parse::<Uri>())
        .collect::<Result<Vec<Uri>, InvalidUri>>()?;

    let result: Result<DatasetsScores, DatabaseError> = web::block(move || {
        // Obtaining a connection from the pool is also a potentially blocking operation.
        // So, it should be called within the `web::block` closure, as well.
        let mut conn = pool.get()?;

        Ok(models::DatasetsScores {
            scores: conn.json_scores(&data.datasets)?,
            aggregations: conn.dimension_aggregates(&data.datasets)?,
        })
    })
    .await
    .map_err(|e| {
        Error::BlockingError(e.into())
    })?;

    match result {
        Ok(scores) => Ok(HttpResponse::Ok()
            .content_type(mime::APPLICATION_JSON)
            .message_body(serde_json::to_string(&scores)?)),
        Err(e) => Err(e.into()),
    }    
}

#[post("/api/assessments")]
async fn assessments(
    accept: web::Header<header::Accept>,
    pool: web::Data<PgPool>,
    body: web::Bytes,
) -> Result<impl Responder, Error> {
    let data = serde_json::from_str::<DatasetsRequest>(from_utf8(&body)?)?;
    // Check that uris are valid, but disregard parsed value.
    let _parsed_dataset_uris = data
        .datasets
        .iter()
        .map(|uri| uri.parse::<Uri>())
        .collect::<Result<Vec<Uri>, InvalidUri>>()?;
    let accept_json_ld = accept
        .0
        .iter()
        .any(|qi| qi.item.to_string() == "application/ld+json");

    let result: Result<String, DatabaseError> = web::block(move || {
        // Obtaining a connection from the pool is also a potentially blocking operation.
        // So, it should be called within the `web::block` closure, as well.
        let mut _conn = pool.get()?;
        
        if accept_json_ld
        {
            // TODO: fetch graphs in jsonld format
            Ok("".to_string())
        } else {
            // TODO: fetch graphs in turtle format
            Ok("".to_string())
        }
    })
    .await
    .map_err(|e| {
        Error::BlockingError(e.into())
    })?;

    match result {
        Ok(graph) => Ok(HttpResponse::Ok()
            .content_type(if accept_json_ld { "application/ld+json" } else { "text/turtle" })
            .message_body(graph)),
        Err(e) => Err(e.into()),
    }    
}

fn app() -> App<
    impl ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<EitherBody<BoxBody>>,
        Error = actix_web::Error,
        Config = (),
        InitError = (),
    >,
> {
    let pool = PgPool::new().unwrap();

    let openapi = serde_yaml::from_str::<OpenApi>(include_str!("../openapi.yaml")).unwrap();
    let cors = Cors::default()
        .allow_any_method()
        .allow_any_header()
        .allow_any_origin()
        .max_age(3600);

    App::new()
        .wrap(cors)
        .app_data(web::PayloadConfig::default().limit(8_388_608))
        .app_data(web::Data::new(pool.clone()))
        .service(ping)
        .service(ready)
        .service(assessment_graph)
        .service(update_assessment)
        .service(assessments)
        .service(scores)
        .service(SwaggerUi::new("/swagger-ui/{_:.*}").url("/openapi.json", openapi.clone()))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .with_current_span(false)
        .init();

    tracing::debug!("Tracing initialized");

    migrate_database().unwrap();

    // Fail if API_KEY missing
    let _ = API_KEY.clone();

    HttpServer::new(move || app().wrap(Logger::default()))
        .bind(("0.0.0.0", 8082))?
        .run()
        .await
}

fn parse_uuid(uuid: String) -> Result<Uuid, Error> {
    Uuid::parse_str(uuid.as_ref()).map_err(|_| Error::InvalidID(uuid))
}

#[cfg(test)]
mod tests {
    use actix_web::{http::header::ContentType, test};
    use http::StatusCode;
    use serde_json::Value;
    use uuid::Uuid;

    use super::*;

    async fn test_get_ok(path: &str) {
        let app = test::init_service(app()).await;
        let req = test::TestRequest::get()
            .insert_header(ContentType::plaintext())
            .uri(path)
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn test_ping() {
        test_get_ok("/ping").await;
    }

    #[actix_web::test]
    async fn test_ready() {
        test_get_ok("/ready").await;
    }

    #[actix_web::test]
    async fn test_404() {
        let uuid = Uuid::parse_str("02f09a3f-1624-3b1d-1337-44eff7708208").unwrap();
        let path = format!("/api/assessments/{}", uuid);

        let app = test::init_service(app()).await;

        let req = test::TestRequest::get()
            .insert_header(ContentType::plaintext())
            .uri(&path)
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn test_post_and_get_scores() {
        let uuid = Uuid::parse_str("02f09a3f-1624-3b1d-8409-44eff7708208").unwrap();
        let path = format!("/api/assessments/{}", uuid);

        let app = test::init_service(app()).await;

        let req = test::TestRequest::post()
            .insert_header(ContentType::json())
            .set_json(include_str!("../tests/post.json"))
            .uri(&path)
            .to_request();
        let resp = test::call_service(&app, req).await;
        //println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let req = test::TestRequest::post()
            .insert_header(ContentType::json())
            .insert_header(("X-API-KEY", "bar"))
            .set_json(include_str!("../tests/post.json"))
            .uri(&path)
            .to_request();
        let resp = test::call_service(&app, req).await;
        //println!("{:?}", resp.response().body());
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let req = test::TestRequest::post()
            .insert_header(ContentType::json())
            .insert_header(("X-API-KEY", "foo"))
            .set_json(serde_json::from_str::<Value>(include_str!("../tests/post.json")).unwrap())
            .uri(&path)
            .to_request();
        let resp = test::call_service(&app, req).await;
        //println!("{:?}", resp.response().body());
        assert!(resp.status().is_success());

        let req = test::TestRequest::get().uri(&path).to_request();
        let bytes = test::call_and_read_body(&app, req).await;
        assert_eq!(
            String::from_utf8(bytes.to_vec()).unwrap(),
            include_str!("../tests/assessment.ttl")
        );

        let req = test::TestRequest::post()
            .insert_header(ContentType::json())
            .set_json(
                serde_json::from_str::<Value>(
                    r#"{
                    "datasets": [
                        "https://dataset.foo"
                    ]
                }"#,
                )
                .unwrap(),
            )
            .uri("/api/scores")
            .to_request();
        let body: Value = test::call_and_read_body_json(&app, req).await;
        assert_eq!(
            body,
            serde_json::from_str::<Value>(include_str!("../tests/score.json")).unwrap()
        );
    }
}
