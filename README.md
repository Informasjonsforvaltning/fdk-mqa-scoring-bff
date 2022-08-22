# fdk-mqa-scoring-api

## Generate types

```
docker run --rm -v "${PWD}:/local" openapitools/openapi-generator-cli sh -c "/usr/local/bin/docker-entrypoint.sh generate -i /local/openapi.yaml -g rust -o /out && rm -rf /local/src/models && mv /out/src/models /local/src/models && chown -R $(id -u):$(id -g) /local/src/models"
```

## Test

Start postgres:

```
docker-compose up -d
```

Migrate database (manually exit after startup):

```
API_KEY=foo POSTGRES_HOST=localhost POSTGRES_PORT=5432 POSTGRES_USERNAME=postgres POSTGRES_PASSWORD=postgres POSTGRES_DB_NAME=mqa cargo r
```

Run tests:

```
API_KEY=foo POSTGRES_HOST=localhost POSTGRES_PORT=5432 POSTGRES_USERNAME=postgres POSTGRES_PASSWORD=postgres POSTGRES_DB_NAME=mqa cargo t
```
