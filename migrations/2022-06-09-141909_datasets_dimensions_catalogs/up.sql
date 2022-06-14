CREATE TABLE IF NOT EXISTS datasets (
    id VARCHAR,
    publisher_id VARCHAR NOT NULL,
    title VARCHAR NOT NULL,
    score_graph VARCHAR NOT NULL,
    score_json VARCHAR NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE dimensions (
    dataset_id VARCHAR NOT NULL,
    title VARCHAR NOT NULL,
    score INT NOT NULL,
    max_score INT NOT NULL,
    PRIMARY KEY (dataset_id, title),
    FOREIGN KEY (dataset_id) REFERENCES datasets (id)
);

CREATE TABLE dataset_catalogs (
    dataset_id VARCHAR NOT NULL,
    catalog_id VARCHAR NOT NULL,
    PRIMARY KEY (dataset_id, catalog_id),
    FOREIGN KEY (dataset_id) REFERENCES datasets (id)
);
