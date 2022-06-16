use super::schema::*;
use diesel::sql_types::Double;

#[derive(Insertable, Queryable, AsChangeset)]
#[table_name = "datasets"]
pub struct Dataset {
    pub id: String,
    pub score_graph: String,
    pub score_json: String,
}

#[derive(Insertable, Queryable, AsChangeset)]
#[table_name = "dimensions"]
pub struct Dimension {
    pub dataset_id: String,
    pub id: String,
    pub score: i32,
    pub max_score: i32,
}

#[derive(QueryableByName)]
#[table_name = "dimensions"]
pub struct DimensionAggregate {
    pub id: String,
    #[sql_type = "Double"]
    pub score: f64,
    #[sql_type = "Double"]
    pub max_score: f64,
}
