use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::database;

#[derive(Error, Debug)]
pub enum Error {
    #[error("dataset with FDK ID '{0}' does not exist")]
    NotFound(Uuid),
    #[error("invalid FDK ID: '{0}'")]
    InvalidID(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error(transparent)]
    DatabaseError(#[from] database::DatabaseError),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    SerdeJsonError(#[from] serde_json::Error),
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        use Error::*;
        match self {
            NotFound(_) => HttpResponse::NotFound().json(ErrorReply::message(self)),
            InvalidID(_) => HttpResponse::BadRequest().json(ErrorReply::error(self)),
            Unauthorized(_) => HttpResponse::Unauthorized().json(ErrorReply::error(self)),
            _ => {
                tracing::error!(
                    error = format!("{:?}", self).as_str(),
                    "error occured when processing request"
                );
                HttpResponse::InternalServerError().json(ErrorReply::error(self))
            }
        }
    }
}

#[derive(Default, Serialize)]
pub struct ErrorReply {
    message: Option<String>,
    error: Option<String>,
}

impl ErrorReply {
    fn message<S: ToString>(message: S) -> Self {
        ErrorReply {
            message: Some(message.to_string()),
            ..Default::default()
        }
    }
    fn error<S: ToString>(error: S) -> Self {
        ErrorReply {
            error: Some(error.to_string()),
            ..Default::default()
        }
    }
}
