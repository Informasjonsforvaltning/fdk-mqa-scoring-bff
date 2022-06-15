table! {
    datasets (id) {
        id -> Varchar,
        publisher_id -> Varchar,
        title -> Varchar,
        score_graph -> Varchar,
        score_json -> Varchar,
    }
}

table! {
    dimensions (dataset_id, id) {
        dataset_id -> Varchar,
        id -> Varchar,
        score -> Int4,
        max_score -> Int4,
    }
}

joinable!(dimensions -> datasets (dataset_id));

allow_tables_to_appear_in_same_query!(datasets, dimensions);
