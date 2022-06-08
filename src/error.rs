use std::convert::Infallible;

use rweb::{hyper::StatusCode, reject::Reject, reply, Json, Rejection, Reply};

use serde::Serialize;
use uuid::Uuid;

use crate::{database, scoring};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("score of dataset with FDK ID '{0}' does not exist")]
    NotFound(Uuid),
    #[error("invalid FDK ID: '{0}'")]
    InvalidID(String),
    #[error(transparent)]
    DatabaseError(#[from] database::DatabaseError),
    #[error(transparent)]
    ScoreError(#[from] scoring::ScoreError),
    #[error(transparent)]
    PoolError(#[from] deadpool_postgres::PoolError),
}

impl Reject for Error {}

#[derive(Default, Serialize)]
pub struct ErrorReply {
    message: Option<String>,
    error: Option<String>,
    #[serde(skip)]
    status: StatusCode,
}

impl ErrorReply {
    pub fn from(error: &Error) -> Self {
        use Error::*;
        match error {
            NotFound(_) => ErrorReply {
                message: Some(error.to_string()),
                status: StatusCode::NOT_FOUND,
                ..Default::default()
            },
            InvalidID(_) => ErrorReply {
                error: Some(error.to_string()),
                status: StatusCode::BAD_REQUEST,
                ..Default::default()
            },
            _ => ErrorReply {
                error: Some(error.to_string()),
                status: StatusCode::INTERNAL_SERVER_ERROR,
                ..Default::default()
            },
        }
    }

    pub async fn recover(r: Rejection) -> Result<impl Reply, Infallible> {
        let error = if let Some(error) = r.find::<Error>() {
            ErrorReply::from(error)
        } else if r.is_not_found() {
            ErrorReply {
                message: Some("not found".to_string()),
                status: StatusCode::NOT_FOUND,
                ..Default::default()
            }
        } else {
            ErrorReply {
                error: Some("unkown error".to_string()),
                status: StatusCode::INTERNAL_SERVER_ERROR,
                ..Default::default()
            }
        };

        let status = error.status;
        Ok(reply::with_status(Json::from(error), status))
    }
}
