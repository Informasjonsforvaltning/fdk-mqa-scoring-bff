CREATE TABLE IF NOT EXISTS dataset_assessments (
    id VARCHAR NOT NULL,
    dataset_uri VARCHAR NOT NULL UNIQUE,
    turtle_assessment VARCHAR NOT NULL,
    jsonld_assessment VARCHAR NOT NULL,
    json_score VARCHAR NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE dimensions (
    dataset_uri VARCHAR NOT NULL,
    id VARCHAR NOT NULL,
    score INT NOT NULL,
    max_score INT NOT NULL,
    PRIMARY KEY (dataset_uri, id),
    FOREIGN KEY (dataset_uri) REFERENCES dataset_assessments (dataset_uri) ON DELETE CASCADE
);
