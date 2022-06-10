CREATE TABLE IF NOT EXISTS datasets (
    id VARCHAR,
    publisher_id VARCHAR NOT NULL,
    title VARCHAR NOT NULL,
    score_graph VARCHAR NOT NULL,
    score_json VARCHAR NOT NULL,
    PRIMARY KEY (id)
)
