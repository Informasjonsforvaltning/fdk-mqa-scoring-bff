table! {
    dataset_assessments (id) {
        id -> Varchar,
        dataset_uri -> Varchar,
        turtle_assessment -> Varchar,
        jsonld_assessment -> Varchar,
        json_score -> Varchar,
    }
}

table! {
    dimensions (dataset_uri, id) {
        dataset_uri -> Varchar,
        id -> Varchar,
        score -> Int4,
        max_score -> Int4,
    }
}

allow_tables_to_appear_in_same_query!(dataset_assessments, dimensions,);
