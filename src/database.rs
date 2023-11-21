use std::collections::HashMap;

use diesel::{
    dsl::any,
    expression_methods::ExpressionMethods,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    result, Connection, PgConnection, QueryDsl, RunQueryDsl,
};
use uuid::Uuid;

use crate::{
    db_models::{DatasetAssessment, Dimension, DimensionAggregate},
    models, schema,
};

pub const MIGRATIONS: diesel_migrations::EmbeddedMigrations = diesel_migrations::embed_migrations!("./migrations");
type DB = diesel::pg::Pg;

fn run_migration(conn: &mut impl diesel_migrations::MigrationHarness<DB>) {
    conn.run_pending_migrations(MIGRATIONS).unwrap();
}

#[derive(thiserror::Error, Debug)]
pub enum DatabaseError {
    #[error("{0}: {1}")]
    ConfigError(&'static str, String),
    #[error(transparent)]
    R2d2Error(#[from] r2d2::Error),
    #[error(transparent)]
    DieselError(#[from] result::Error),
    #[error(transparent)]
    DieselConnectionError(#[from] diesel::ConnectionError),
    #[error(transparent)]
    DieselMigrationError(#[from] diesel_migrations::MigrationError),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

fn var(key: &'static str) -> Result<String, DatabaseError> {
    std::env::var(key).map_err(|e| DatabaseError::ConfigError(key, e.to_string()))
}

fn database_url() -> Result<String, DatabaseError> {
    let host = var("POSTGRES_HOST")?;
    let port = var("POSTGRES_PORT")?
        .parse::<u16>()
        .map_err(|e| DatabaseError::ConfigError("POSTGRES_PORT", e.to_string()))?;
    let user = var("POSTGRES_USERNAME")?;
    let password = var("POSTGRES_PASSWORD")?;
    let dbname = var("POSTGRES_DB_NAME")?;
    let url = format!("postgres://{user}:{password}@{host}:{port}/{dbname}");

    Ok(url)
}

pub fn migrate_database() -> Result<(), DatabaseError> {
    let url = database_url()?;
    let mut conn = PgConnection::establish(&url)?;
    run_migration(&mut conn);

    Ok(())
}

#[derive(Clone)]
pub struct PgPool(Pool<ConnectionManager<PgConnection>>);

impl PgPool {
    pub fn new() -> Result<Self, DatabaseError> {
        let url = database_url()?;
        let manager = ConnectionManager::new(url);
        let pool = Pool::builder()
            .max_size(5)
            .test_on_check_out(true)
            .build(manager)
            .expect("Could not create a connection pool");
        Ok(PgPool(pool))
    }

    pub fn get(&self) -> Result<PgConn, DatabaseError> {
        Ok(PgConn(self.0.get()?))
    }
}

pub struct PgConn(PooledConnection<ConnectionManager<PgConnection>>);

impl PgConn {
    pub fn test_connection(&mut self) -> Result<(), DatabaseError> {
        use schema::dimensions::dsl;
        
        let _: i64 = dsl::dimensions.select(diesel::dsl::count(dsl::id)).first(&mut self.0)?;
        Ok(())
    }

    pub fn store_dataset(&mut self, assessment: DatasetAssessment) -> Result<(), DatabaseError> {
        use schema::dataset_assessments::dsl;

        diesel::insert_into(dsl::dataset_assessments)
            .values(&assessment)
            .on_conflict(dsl::id)
            .do_update()
            .set(&assessment)
            .execute(&mut self.0)?;

        Ok(())
    }

    pub fn store_dimension(&mut self, dimension: Dimension) -> Result<(), DatabaseError> {
        use schema::dimensions::dsl;

        diesel::insert_into(dsl::dimensions)
            .values(&dimension)
            .on_conflict((dsl::dataset_uri, dsl::id))
            .do_update()
            .set(&dimension)
            .execute(&mut self.0)?;

        Ok(())
    }

    pub fn drop_dataset_dimensions(&mut self, dataset_uri: &str) -> Result<(), DatabaseError> {
        use schema::dimensions::dsl;

        diesel::delete(dsl::dimensions)
            .filter(dsl::dataset_uri.eq(dataset_uri))
            .execute(&mut self.0)?;

        Ok(())
    }

    pub fn turtle_assessment(
        &mut self,
        dataset_assessment: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        use schema::dataset_assessments::dsl;

        match dsl::dataset_assessments
            .filter(dsl::id.eq(dataset_assessment.to_string()))
            .select(dsl::turtle_assessment)
            .first(&mut self.0)
        {
            Ok(assessment) => Ok(Some(assessment)),
            Err(result::Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn jsonld_assessment(
        &mut self,
        dataset_assessment: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        use schema::dataset_assessments::dsl;

        match dsl::dataset_assessments
            .filter(dsl::id.eq(dataset_assessment.to_string()))
            .select(dsl::jsonld_assessment)
            .first(&mut self.0)
        {
            Ok(assessment) => Ok(Some(assessment)),
            Err(result::Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// NOTE!! Ensure that URIs are valid before calling this.
    pub fn json_scores(
        &mut self,
        dataset_uris: &Vec<String>,
    ) -> Result<HashMap<String, models::DatasetScore>, DatabaseError> {
        use schema::dataset_assessments::dsl;

        let uris = dataset_uris
            .iter()
            .map(|uri| uri.to_string())
            .collect::<Vec<String>>();

        let rows: Vec<(String, String)> = dsl::dataset_assessments
            .filter(dsl::dataset_uri.eq(any(uris)))
            .select((dsl::dataset_uri, dsl::json_score))
            .get_results(&mut self.0)?;

        let dataset_scores = rows
            .into_iter()
            .map(|(dataset_uri, json)| Ok((dataset_uri, serde_json::from_str(&json)?)))
            .collect::<Result<HashMap<String, models::DatasetScore>, DatabaseError>>()?;

        Ok(dataset_scores)
    }

    /// NOTE!! Ensure that URIs are valid before calling this.
    pub fn dimension_aggregates(
        &mut self,
        dataset_uris: &Vec<String>,
    ) -> Result<Vec<models::DimensionAggregate>, DatabaseError> {
        let q = format!(
            "SELECT id, AVG(score)::float8 AS score, AVG(max_score)::float8 AS max_score
             FROM dimensions WHERE dataset_uri in ({}) GROUP BY id",
            dataset_uris
                .iter()
                .map(|uri| format!("'{uri}'"))
                .collect::<Vec<String>>()
                .join(",")
        );
        let aggregates: Vec<DimensionAggregate> =
            diesel::dsl::sql_query(q).get_results(&mut self.0)?;

        Ok(aggregates
            .into_iter()
            .map(
                |DimensionAggregate {
                     id,
                     score,
                     max_score,
                 }| models::DimensionAggregate {
                    id,
                    score,
                    max_score,
                },
            )
            .collect())
    }
}
