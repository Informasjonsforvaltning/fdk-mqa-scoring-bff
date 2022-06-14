use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRequest {
    pub graph: String,
    pub scores: Scores,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Scores {
    pub dataset: Score,
    distributions: Vec<Score>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Score {
    name: String,
    pub dimensions: Vec<DimensionScore>,
    score: u64,
    max_score: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DimensionScore {
    pub name: String,
    metrics: Vec<MetricScore>,
    pub score: u64,
    pub max_score: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricScore {
    metric: String,
    score: u64,
    is_scored: bool,
    max_score: u64,
}
