use super::schema::*;

#[derive(Insertable, Queryable, AsChangeset)]
#[diesel(table_name = datasets)]
pub struct Dataset {
    pub id: String,
    pub score_graph: String,
    pub score_json: String,
}

#[derive(Insertable, Queryable, AsChangeset)]
#[diesel(table_name = dimensions)]
pub struct Dimension {
    pub dataset_id: String,
    pub title: String,
    pub score: i32,
    pub max_score: i32,
}
