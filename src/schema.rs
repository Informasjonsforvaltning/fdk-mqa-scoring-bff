table! {
    dataset_catalogs (dataset_id, catalog_id) {
        dataset_id -> Varchar,
        catalog_id -> Varchar,
    }
}

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
    dimensions (dataset_id, title) {
        dataset_id -> Varchar,
        title -> Varchar,
        score -> Int4,
        max_score -> Int4,
    }
}

joinable!(dataset_catalogs -> datasets (dataset_id));
joinable!(dimensions -> datasets (dataset_id));

allow_tables_to_appear_in_same_query!(dataset_catalogs, datasets, dimensions,);
