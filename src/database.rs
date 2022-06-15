use diesel::{
    expression_methods::ExpressionMethods,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    result, Connection, PgConnection, QueryDsl, RunQueryDsl,
};
use uuid::Uuid;

use crate::{
    models::{Dataset, Dimension},
    schema,
};

diesel_migrations::embed_migrations!("./migrations");

#[derive(thiserror::Error, Debug)]
pub enum DatabaseError {
    #[error("{0}: {1}")]
    ConfigError(&'static str, String),
    #[error(transparent)]
    R2d2Error(#[from] r2d2::Error),
    #[error(transparent)]
    DieselError(#[from] diesel::result::Error),
    #[error(transparent)]
    DieselConnectionError(#[from] diesel::ConnectionError),
    #[error(transparent)]
    DieselMigrationError(#[from] diesel_migrations::RunMigrationsError),
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
    let conn = PgConnection::establish(&url)?;
    embedded_migrations::run(&conn)?;

    Ok(())
}

#[derive(Clone)]
pub struct PgPool(Pool<ConnectionManager<PgConnection>>);

impl PgPool {
    pub fn new() -> Result<Self, DatabaseError> {
        let url = database_url()?;
        let manager = ConnectionManager::new(url);
        let pool = Pool::builder().test_on_check_out(true).build(manager)?;
        Ok(PgPool(pool))
    }

    pub fn get(&self) -> Result<PgConn, DatabaseError> {
        Ok(PgConn(self.0.get()?))
    }
}

pub struct PgConn(PooledConnection<ConnectionManager<PgConnection>>);

impl PgConn {
    pub fn test_connection(&self) -> Result<(), DatabaseError> {
        // TODO: test connection
        Ok(())
    }

    pub fn store_dataset(&mut self, dataset: Dataset) -> Result<(), DatabaseError> {
        use schema::datasets::dsl;

        diesel::insert_into(dsl::datasets)
            .values(&dataset)
            .on_conflict(dsl::id)
            .do_update()
            .set(&dataset)
            .execute(&mut self.0)?;

        Ok(())
    }

    pub fn store_dimension(&mut self, dimension: Dimension) -> Result<(), DatabaseError> {
        use schema::dimensions::dsl;

        diesel::insert_into(dsl::dimensions)
            .values(&dimension)
            .on_conflict((dsl::dataset_id, dsl::id))
            .do_update()
            .set(&dimension)
            .execute(&mut self.0)?;

        Ok(())
    }

    pub fn drop_dimensions(&mut self, id: Uuid) -> Result<(), DatabaseError> {
        use schema::dimensions::dsl;

        diesel::delete(dsl::dimensions)
            .filter(dsl::dataset_id.eq(id.to_string()))
            .execute(&mut self.0)?;

        Ok(())
    }

    pub fn get_score_graph_by_id(&mut self, id: Uuid) -> Result<Option<String>, DatabaseError> {
        use schema::datasets::dsl;

        match dsl::datasets
            .filter(dsl::id.eq(id.to_string()))
            .select(dsl::score_graph)
            .first(&mut self.0)
        {
            Ok(graph) => Ok(Some(graph)),
            Err(result::Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_score_json_by_id(&mut self, id: Uuid) -> Result<Option<String>, DatabaseError> {
        use schema::datasets::dsl;

        match dsl::datasets
            .filter(dsl::id.eq(id.to_string()))
            .select(dsl::score_json)
            .first(&mut self.0)
        {
            Ok(graph) => Ok(Some(graph)),
            Err(result::Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
