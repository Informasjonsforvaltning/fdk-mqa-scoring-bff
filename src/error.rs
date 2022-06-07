use actix_web::{HttpResponse, ResponseError};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

use crate::{database, score};

#[derive(Error, Debug)]
pub enum Error {
    #[error("score of dataset with FDK ID '{0}' does not exist")]
    NotFound(Uuid),
    #[error("invalid FDK ID: '{0}'")]
    InvalidID(String),
    #[error(transparent)]
    DatabaseError(#[from] database::DatabaseError),
    #[error(transparent)]
    ScoreError(#[from] score::ScoreError),
    #[error(transparent)]
    PoolError(#[from] deadpool_postgres::PoolError),
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        use Error::*;
        match self {
            NotFound(_) => HttpResponse::NotFound().json(json!({"message": self.to_string()})),
            InvalidID(_) => HttpResponse::BadRequest().json(json!({"error": self.to_string()})),
            _ => HttpResponse::InternalServerError().json(json!({"error": self.to_string()})),
        }
    }
}
