use oxigraph::{
    io::GraphFormat,
    model::{self, GraphNameRef, NamedNode, NamedNodeRef, Quad, Subject, Term},
    sparql::{self, QueryResults, QuerySolution},
    store::{self, StorageError, Store},
};
use serde::Serialize;
use thiserror::Error;

use crate::vocab::{dcat, dcat_mqa, dqv, rdf_syntax};

#[derive(Error, Debug)]
pub enum ScoreError {
    #[error(transparent)]
    LoaderError(#[from] store::LoaderError),
    #[error(transparent)]
    SerializerError(#[from] store::SerializerError),
    #[error(transparent)]
    StorageError(#[from] store::StorageError),
    #[error(transparent)]
    EvaluationError(#[from] sparql::EvaluationError),
    #[error(transparent)]
    QueryError(#[from] sparql::QueryError),
    #[error(transparent)]
    IriParseError(#[from] model::IriParseError),
    #[error("{0}")]
    String(String),
}

impl From<&str> for ScoreError {
    fn from(e: &str) -> Self {
        Self::String(e.to_string())
    }
}

impl From<String> for ScoreError {
    fn from(e: String) -> Self {
        Self::String(e)
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Scores {
    dataset: Score,
    distributions: Vec<Score>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Score {
    name: String,
    dimensions: Vec<DimensionScore>,
    score: u64,
    max_score: u64,
}

impl Score {
    fn new(name: String, dimensions: Vec<DimensionScore>) -> Self {
        Self {
            score: dimensions.iter().map(|d| d.score).sum(),
            max_score: dimensions.iter().map(|d| d.max_score).sum(),
            name,
            dimensions,
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct DimensionScore {
    name: String,
    metrics: Vec<MetricScore>,
    score: u64,
    max_score: u64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MetricScore {
    metric: String,
    score: u64,
    is_scored: bool,
    max_score: u64,
}

pub struct ScoreGraph(Store);

impl ScoreGraph {
    /// Loads graph from string.
    pub fn parse<G: ToString>(graph: G) -> Result<Self, ScoreError> {
        let store = Store::new()?;
        store.load_graph(
            graph.to_string().as_ref(),
            GraphFormat::Turtle,
            GraphNameRef::DefaultGraph,
            None,
        )?;
        Ok(Self(store))
    }

    /// Retrieves a single named or blank dataset nodes.
    pub fn dataset(&self) -> Result<NamedNode, ScoreError> {
        self.0
            .quads_for_pattern(
                None,
                Some(rdf_syntax::TYPE),
                Some(dcat::DATASET.into()),
                None,
            )
            .map(named_quad_subject)
            .next()
            .unwrap_or(Err("score graph has no datasets".into()))
    }

    /// Retrieves all named or blank distribution nodes.
    pub fn distributions(&self) -> Result<Vec<NamedNode>, ScoreError> {
        self.0
            .quads_for_pattern(None, Some(dcat::DISTRIBUTION.into()), None, None)
            .map(named_quad_object)
            .collect()
    }

    /// Parse score for dataset and all distributions.
    pub fn score(&self) -> Result<Scores, ScoreError> {
        let dataset = self.dataset()?;
        let dimensions = self.node_score(dataset.as_ref())?;
        let dataset_score = Score::new(dataset.as_str().to_string(), dimensions);

        let distributions = self.distributions()?;
        let distribution_scores: Vec<Score> = distributions
            .into_iter()
            .map(|dist| {
                let dimensions = self.node_score(dist.as_ref())?;
                Ok(Score::new(dist.as_str().to_string(), dimensions))
            })
            .collect::<Result<_, ScoreError>>()?;

        Ok(Scores {
            dataset: dataset_score,
            distributions: distribution_scores,
        })
    }

    pub fn node_score(&self, node: NamedNodeRef) -> Result<Vec<DimensionScore>, ScoreError> {
        let query = format!(
            "
            SELECT ?dimension ?metric ?score ?max_score
            WHERE {{
                {node} {} ?measurement .
                ?measurement {} ?metric .
                ?metric {} ?dimension .
                ?metric {} ?max_score .
                OPTIONAL {{ ?measurement {} ?score }}
            }}
            ORDER BY ?dimension
        ",
            dqv::HAS_QUALITY_MEASUREMENT,
            dqv::IS_MEASUREMENT_OF,
            dqv::IN_DIMENSION,
            dcat_mqa::TRUE_SCORE,
            dcat_mqa::SCORE,
        );

        let mut dimensions = Vec::<DimensionScore>::new();

        for qs in execute_query(&self.0, &query)?.into_iter() {
            let dimension = match qs.get("dimension") {
                Some(Term::NamedNode(node)) => Ok(node.as_str().to_string()),
                _ => Err("unable to get dimension"),
            }?;
            let metric = match qs.get("metric") {
                Some(Term::NamedNode(node)) => Ok(node.as_str().to_string()),
                _ => Err("unable to get metric"),
            }?;
            let score = match qs.get("score") {
                Some(Term::Literal(l)) => match l.value().parse::<u64>() {
                    Ok(v) => Ok(Some(v)),
                    Err(_) => Err(format!("unable to parse score: {}", l.value())),
                },
                _ => Ok(None),
            }?;
            let max_score = match qs.get("max_score") {
                Some(Term::Literal(l)) => match l.value().parse::<u64>() {
                    Ok(v) => Ok(v),
                    Err(_) => Err(format!(
                        "unable to parse metric potential score: {}",
                        l.value()
                    )),
                },
                _ => Err("unable to get metric potential score".into()),
            }?;

            let metric_score = MetricScore {
                metric,
                score: score.unwrap_or(0),
                is_scored: score.is_some(),
                max_score,
            };

            // Group metrics by dimension. Updates last dimension in returned list,
            // as long as iterated metric is within same dimension as previous.
            // NOTE: requires `ORDER BY ?dimension`
            match dimensions.last_mut() {
                Some(DimensionScore {
                    name: dimension_name,
                    metrics: dimension_metrics,
                    max_score: dimension_max_score,
                    score: dimension_score,
                }) if &dimension.as_str() == dimension_name => {
                    let index = dimension_metrics
                        .binary_search_by(|m| m.metric.cmp(&metric_score.metric))
                        .unwrap_or_else(|e| e);
                    dimension_metrics.insert(index, metric_score);
                    *dimension_score += score.unwrap_or(0);
                    *dimension_max_score += max_score;
                }
                _ => dimensions.push(DimensionScore {
                    name: dimension,
                    metrics: vec![metric_score],
                    score: score.unwrap_or(0),
                    max_score,
                }),
            }
        }

        Ok(dimensions)
    }
}

// Executes SPARQL query on store.
fn execute_query(store: &Store, q: &str) -> Result<Vec<QuerySolution>, ScoreError> {
    match store.query(q) {
        Ok(QueryResults::Solutions(solutions)) => match solutions.collect() {
            Ok(vec) => Ok(vec),
            Err(e) => Err(e.into()),
        },
        Err(e) => Err(e.into()),
        _ => Err("unable to execute query".into()),
    }
}

// Attemts to extract quad subject as named node.
fn named_quad_subject(result: Result<Quad, StorageError>) -> Result<NamedNode, ScoreError> {
    match result?.subject {
        Subject::NamedNode(node) => Ok(node),
        _ => Err("unable to get named quad object".into()),
    }
}

// Attemts to extract quad object as named node.
fn named_quad_object(result: Result<Quad, StorageError>) -> Result<NamedNode, ScoreError> {
    match result?.object {
        Term::NamedNode(node) => Ok(node),
        _ => Err("unable to get named quad object".into()),
    }
}

#[cfg(test)]
mod tests {
    use crate::score::{DimensionScore, MetricScore, Score, Scores};

    use super::ScoreGraph;

    #[test]
    fn score() {
        let score = ScoreGraph::parse(
            r#"
            @prefix dcatno-mqa: <https://data.norge.no/vocabulary/dcatno-mqa#> .
            @prefix dqv:        <http://www.w3.org/ns/dqv#> .
            dcatno-mqa:accessibility
                a                       dqv:Dimension .
            dcatno-mqa:interoperability
                a                       dqv:Dimension .
            dcatno-mqa:accessUrlStatusCode
                a                       dqv:Metric ;
                dqv:inDimension         dcatno-mqa:accessibility .
            dcatno-mqa:downloadUrlAvailability
                a                       dqv:Metric ;
                dqv:inDimension         dcatno-mqa:accessibility .
            dcatno-mqa:formatAvailability
                a                       dqv:Metric ;
                dqv:inDimension         dcatno-mqa:interoperability .
            
            
            @prefix dcatno-mqa: <https://data.norge.no/vocabulary/dcatno-mqa#> .
            @prefix xsd:        <http://www.w3.org/2001/XMLSchema#> .
            dcatno-mqa:accessUrlStatusCode
                dcatno-mqa:trueScore            "50"^^xsd:integer .
            dcatno-mqa:downloadUrlAvailability
                dcatno-mqa:trueScore            "20"^^xsd:integer .
            dcatno-mqa:formatAvailability
                dcatno-mqa:trueScore            "20"^^xsd:integer .


            <https://dataset.foo> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:f7e095b2d5f86a95777560c21e6b02f .
            <https://dataset.foo> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:39ec948902916c2a986e747f931ce6a2 .
            <https://dataset.foo> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:75ffb8f962328784bb75b4b6d6a59280 .
            <https://dataset.foo> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:92000c3a53f435a487d98023b56c466c .
            <https://dataset.foo> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:ad6bdc8fad6782d018cf799c44e9d3be .
            <https://dataset.foo> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:f602b0499b0ec9a9b4751783aae56d43 .
            <https://dataset.foo> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dcat#Dataset> .
            <https://dataset.foo> <http://www.w3.org/ns/dcat#distribution> <https://distribution.b> .
            <https://dataset.foo> <http://www.w3.org/ns/dcat#distribution> <https://distribution.a> .
            <https://distribution.b> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:588470fc05e9e76fe6a6fc1ab331462 .
            <https://distribution.b> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:8ead2c477b7e37a67e198c0cb3ae3be2 .
            <https://distribution.b> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:a710268239af421b9e6d62d0d6981133 .
            <https://distribution.b> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:bbb83f4f2f0af3c306070dcff610e2a1 .
            <https://distribution.b> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:bf76b489f7ba87abb4a388f2b9a393c6 .
            <https://distribution.b> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:d0f2fff1b2d3b30541caa9b45cefae64 .
            <https://distribution.a> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:44ad6cbfb668280b1ca6b4a908a69c7e .
            <https://distribution.a> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:5ac15e64d59eb0c956e1dd6b1b11bbe7 .
            <https://distribution.a> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:60a0f8220f2d80ece7ef943e1a15f427 .
            <https://distribution.a> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:e3659aa9fc6cb234fa924d25ad6da0c2 .
            <https://distribution.a> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:e5432076e5f0c8e16e9de66469568851 .
            <https://distribution.a> <http://www.w3.org/ns/dqv#hasQualityMeasurement> _:fcf5e35f314519cb0e63b187e3d7d1b8 .
            _:588470fc05e9e76fe6a6fc1ab331462 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:588470fc05e9e76fe6a6fc1ab331462 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#scoring> .
            _:588470fc05e9e76fe6a6fc1ab331462 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.b> .
            _:f7e095b2d5f86a95777560c21e6b02f <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:f7e095b2d5f86a95777560c21e6b02f <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#formatAvailability> .
            _:f7e095b2d5f86a95777560c21e6b02f <http://www.w3.org/ns/dqv#computedOn> <https://dataset.foo> .
            _:34ef6f3b1311e2cab7b44c386834a6d5 <https://data.norge.no/vocabulary/dcatno-mqa#score> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:39ec948902916c2a986e747f931ce6a2 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:39ec948902916c2a986e747f931ce6a2 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#interoperabilityScoring> .
            _:39ec948902916c2a986e747f931ce6a2 <http://www.w3.org/ns/dqv#computedOn> <https://dataset.foo> .
            _:3a25c2df87cfc60f69f4c6f8ac2b9dde <http://www.w3.org/ns/dqv#value> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:431474afccc32f342066a7b46e9bc9be <http://www.w3.org/ns/dqv#value> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:44ad6cbfb668280b1ca6b4a908a69c7e <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:44ad6cbfb668280b1ca6b4a908a69c7e <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#accessibilityScoring> .
            _:44ad6cbfb668280b1ca6b4a908a69c7e <http://www.w3.org/ns/dqv#computedOn> <https://distribution.a> .
            _:474982b4ddec9ff68a44db3b0f206e6e <https://data.norge.no/vocabulary/dcatno-mqa#score> "50"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:47eb96b9255337c6334d45616906d2a8 <https://data.norge.no/vocabulary/dcatno-mqa#score> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:486acbc668d462d1a34923b78f2cd111 <http://www.w3.org/ns/dqv#value> "50"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:5ac15e64d59eb0c956e1dd6b1b11bbe7 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:5ac15e64d59eb0c956e1dd6b1b11bbe7 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#downloadUrlAvailability> .
            _:5ac15e64d59eb0c956e1dd6b1b11bbe7 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.a> .
            _:60a0f8220f2d80ece7ef943e1a15f427 <http://www.w3.org/ns/dqv#value> "false"^^<http://www.w3.org/2001/XMLSchema#boolean> .
            _:60a0f8220f2d80ece7ef943e1a15f427 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:60a0f8220f2d80ece7ef943e1a15f427 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#formatAvailability> .
            _:60a0f8220f2d80ece7ef943e1a15f427 <https://data.norge.no/vocabulary/dcatno-mqa#score> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:631f465b8825c07d3235308a35685fd9 <http://www.w3.org/ns/dqv#value> "20"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:75ffb8f962328784bb75b4b6d6a59280 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:75ffb8f962328784bb75b4b6d6a59280 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#scoring> .
            _:75ffb8f962328784bb75b4b6d6a59280 <http://www.w3.org/ns/dqv#computedOn> <https://dataset.foo> .
            _:7951ff5c72beb23c02a99af5df8155be <http://www.w3.org/ns/dqv#value> "70"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:822b6d16c0cbe8d49269016a4d0574a4 <http://www.w3.org/ns/dqv#value> "50"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:8ead2c477b7e37a67e198c0cb3ae3be2 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:8ead2c477b7e37a67e198c0cb3ae3be2 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#accessibilityScoring> .
            _:8ead2c477b7e37a67e198c0cb3ae3be2 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.b> .
            _:92000c3a53f435a487d98023b56c466c <http://www.w3.org/ns/dqv#value> "true"^^<http://www.w3.org/2001/XMLSchema#boolean> .
            _:92000c3a53f435a487d98023b56c466c <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:92000c3a53f435a487d98023b56c466c <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#downloadUrlAvailability> .
            _:92000c3a53f435a487d98023b56c466c <https://data.norge.no/vocabulary/dcatno-mqa#score> "20"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:9cba6960fef12e734465c710d024393c <http://www.w3.org/ns/dqv#value> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:a710268239af421b9e6d62d0d6981133 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:a710268239af421b9e6d62d0d6981133 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#downloadUrlAvailability> .
            _:a710268239af421b9e6d62d0d6981133 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.b> .
            _:ad6bdc8fad6782d018cf799c44e9d3be <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:ad6bdc8fad6782d018cf799c44e9d3be <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#accessibilityScoring> .
            _:ad6bdc8fad6782d018cf799c44e9d3be <http://www.w3.org/ns/dqv#computedOn> <https://dataset.foo> .
            _:bbb83f4f2f0af3c306070dcff610e2a1 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:bbb83f4f2f0af3c306070dcff610e2a1 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#accessUrlStatusCode> .
            _:bbb83f4f2f0af3c306070dcff610e2a1 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.b> .
            _:bbebfbc6bfe5bfc018465c261bb6b351 <https://data.norge.no/vocabulary/dcatno-mqa#score> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:bf76b489f7ba87abb4a388f2b9a393c6 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:bf76b489f7ba87abb4a388f2b9a393c6 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#interoperabilityScoring> .
            _:bf76b489f7ba87abb4a388f2b9a393c6 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.b> .
            _:cd361a6a0c7d1b82da273de63ee27926 <https://data.norge.no/vocabulary/dcatno-mqa#score> "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:d0f2fff1b2d3b30541caa9b45cefae64 <http://www.w3.org/ns/dqv#value> "true"^^<http://www.w3.org/2001/XMLSchema#boolean> .
            _:d0f2fff1b2d3b30541caa9b45cefae64 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:d0f2fff1b2d3b30541caa9b45cefae64 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#formatAvailability> .
            _:d0f2fff1b2d3b30541caa9b45cefae64 <https://data.norge.no/vocabulary/dcatno-mqa#score> "20"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:e3659aa9fc6cb234fa924d25ad6da0c2 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:e3659aa9fc6cb234fa924d25ad6da0c2 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#scoring> .
            _:e3659aa9fc6cb234fa924d25ad6da0c2 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.a> .
            _:e5432076e5f0c8e16e9de66469568851 <http://www.w3.org/ns/dqv#value> "true"^^<http://www.w3.org/2001/XMLSchema#boolean> .
            _:e5432076e5f0c8e16e9de66469568851 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:e5432076e5f0c8e16e9de66469568851 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#accessUrlStatusCode> .
            _:e5432076e5f0c8e16e9de66469568851 <https://data.norge.no/vocabulary/dcatno-mqa#score> "50"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:eb5ca9f5b8424f2ed38e884a1a465d87 <http://www.w3.org/ns/dqv#value> "20"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:f27d3fe788f1e4b8c45d04c9da996217 <http://www.w3.org/ns/dqv#value> "70"^^<http://www.w3.org/2001/XMLSchema#integer> .
            _:f602b0499b0ec9a9b4751783aae56d43 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:f602b0499b0ec9a9b4751783aae56d43 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#accessUrlStatusCode> .
            _:f602b0499b0ec9a9b4751783aae56d43 <http://www.w3.org/ns/dqv#computedOn> <https://dataset.foo> .
            _:fcf5e35f314519cb0e63b187e3d7d1b8 <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dqv#QualityMeasurement> .
            _:fcf5e35f314519cb0e63b187e3d7d1b8 <http://www.w3.org/ns/dqv#isMeasurementOf> <https://data.norge.no/vocabulary/dcatno-mqa#interoperabilityScoring> .
            _:fcf5e35f314519cb0e63b187e3d7d1b8 <http://www.w3.org/ns/dqv#computedOn> <https://distribution.a> .
            "#,
        )
        .unwrap()
        .score()
        .unwrap();

        assert_eq!(score, Scores {
            dataset: Score {
                name: "https://dataset.foo".to_string(),
                dimensions: vec![
                    DimensionScore {
                        name: "https://data.norge.no/vocabulary/dcatno-mqa#accessibility".to_string(),
                        metrics: vec![
                            MetricScore {
                                metric: "https://data.norge.no/vocabulary/dcatno-mqa#accessUrlStatusCode".to_string(),
                                score: 0,
                                is_scored: false,
                                max_score: 50,
                            },
                            MetricScore {
                                metric: "https://data.norge.no/vocabulary/dcatno-mqa#downloadUrlAvailability".to_string(),
                                score: 20,
                                is_scored: true,
                                max_score: 20,
                            },
                        ],
                        score: 20,
                        max_score: 70,
                    },
                    DimensionScore {
                        name: "https://data.norge.no/vocabulary/dcatno-mqa#interoperability".to_string(),
                        metrics: vec![
                            MetricScore {
                                metric: "https://data.norge.no/vocabulary/dcatno-mqa#formatAvailability".to_string(),
                                score: 0,
                                is_scored: false,
                                max_score: 20,
                            },
                        ],
                        score: 0,
                        max_score: 20,
                    },
                ],
                score: 20,
                max_score: 90,
            },
            distributions: vec![
                Score {
                    name: "https://distribution.b".to_string(),
                    dimensions: vec![
                        DimensionScore {
                            name: "https://data.norge.no/vocabulary/dcatno-mqa#accessibility".to_string(),
                            metrics: vec![
                                MetricScore {
                                    metric: "https://data.norge.no/vocabulary/dcatno-mqa#accessUrlStatusCode".to_string(),
                                    score: 0,
                                    is_scored: false,
                                    max_score: 50,
                                },
                                MetricScore {
                                    metric: "https://data.norge.no/vocabulary/dcatno-mqa#downloadUrlAvailability".to_string(),
                                    score: 0,
                                    is_scored: false,
                                    max_score: 20,
                                },
                            ],
                            score: 0,
                            max_score: 70,
                        },
                        DimensionScore {
                            name: "https://data.norge.no/vocabulary/dcatno-mqa#interoperability".to_string(),
                            metrics: vec![
                                MetricScore {
                                    metric: "https://data.norge.no/vocabulary/dcatno-mqa#formatAvailability".to_string(),
                                    score: 20,
                                    is_scored: true,
                                    max_score: 20,
                                },
                            ],
                            score: 20,
                            max_score: 20,
                        },
                    ],
                    score: 20,
                    max_score: 90,
                },
                Score {
                    name: "https://distribution.a".to_string(),
                    dimensions: vec![
                        DimensionScore {
                            name: "https://data.norge.no/vocabulary/dcatno-mqa#accessibility".to_string(),
                            metrics: vec![
                                MetricScore {
                                    metric: "https://data.norge.no/vocabulary/dcatno-mqa#accessUrlStatusCode".to_string(),
                                    score: 50,
                                    is_scored: true,
                                    max_score: 50,
                                },
                                MetricScore {
                                    metric: "https://data.norge.no/vocabulary/dcatno-mqa#downloadUrlAvailability".to_string(),
                                    score: 0,
                                    is_scored: false,
                                    max_score: 20,
                                },
                            ],
                            score: 50,
                            max_score: 70,
                        },
                        DimensionScore {
                            name: "https://data.norge.no/vocabulary/dcatno-mqa#interoperability".to_string(),
                            metrics: vec![
                                MetricScore {
                                    metric: "https://data.norge.no/vocabulary/dcatno-mqa#formatAvailability".to_string(),
                                    score: 0,
                                    is_scored: true,
                                    max_score: 20,
                                },
                            ],
                            score: 0,
                            max_score: 20,
                        },
                    ],
                    score: 50,
                    max_score: 90,
                },
            ],
        });
    }
}
