CREATE TABLE dataset_catalogs (
    dataset_id VARCHAR NOT NULL,
    catalog_id VARCHAR NOT NULL,
    PRIMARY KEY (dataset_id, catalog_id),
    FOREIGN KEY (dataset_id) REFERENCES datasets (id)
)
