CREATE TABLE IF NOT EXISTS datasets (
    id VARCHAR,
    score_graph VARCHAR NOT NULL,
    score_json VARCHAR NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE dimensions (
    dataset_id VARCHAR NOT NULL,
    id VARCHAR NOT NULL,
    score INT NOT NULL,
    max_score INT NOT NULL,
    PRIMARY KEY (dataset_id, id),
    FOREIGN KEY (dataset_id) REFERENCES datasets (id)
);
