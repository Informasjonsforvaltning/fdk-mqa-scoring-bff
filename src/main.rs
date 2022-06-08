use database::connection_pool;
use deadpool_postgres::{Client, Pool};
use error::ErrorReply;
use rweb::{get, openapi, openapi_docs, rt::tokio, serve, Filter, Json, Rejection, Reply};
use uuid::Uuid;

use crate::{
    database::{get_graph_by_id, test_connection},
    error::Error,
    scoring::{ScoreGraph, Scores},
};

mod database;
mod error;
mod scoring;
mod vocab;

#[get("/ping")]
async fn ping(#[data] pool: Pool) -> Result<impl Reply, Rejection> {
    async fn ping(pool: Pool) -> Result<impl Reply, Error> {
        let client: Client = pool.get().await?;
        test_connection(&client).await?;
        Ok("pong")
    }

    Ok(ping(pool).await?)
}

#[get("/ready")]
fn ready() -> impl Reply {
    "ok"
}

#[get("/score/{id}")]
#[openapi(tags("score"))]
#[openapi(id = "score")]
#[openapi(summary = "Get dataset score")]
async fn score(#[data] pool: Pool, id: String) -> Result<Json<Scores>, Rejection> {
    async fn score(pool: Pool, id: String) -> Result<Json<Scores>, Error> {
        let uuid = Uuid::parse_str(id.as_ref()).map_err(|_| Error::InvalidID(id))?;

        let client: Client = pool.get().await?;

        if let Some(graph_string) = get_graph_by_id(&client, uuid).await? {
            let scores = ScoreGraph::parse(graph_string)?.score()?;
            Ok(Json::from(scores))
        } else {
            Err(Error::NotFound(uuid))
        }
    }

    Ok(score(pool, id).await?)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();

    let pool = connection_pool().unwrap();

    let (spec, filter) = openapi::spec().build(|| score(pool.clone()));
    serve(
        filter
            .or(ping(pool.clone()))
            .or(ready())
            .or(openapi_docs(spec))
            .recover(ErrorReply::recover),
    )
    .run(([0, 0, 0, 0], 8080))
    .await;
}
