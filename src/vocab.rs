#[macro_export]
macro_rules! n {
    ($iri:expr) => {
        oxigraph::model::NamedNodeRef::new_unchecked($iri)
    };
}

type N = oxigraph::model::NamedNodeRef<'static>;

pub mod dcat {
    use super::N;

    pub const DATASET: N = n!("http://www.w3.org/ns/dcat#Dataset");
    pub const DISTRIBUTION: N = n!("http://www.w3.org/ns/dcat#distribution");
}

pub mod dqv {
    use super::N;

    pub const IN_DIMENSION: N = n!("http://www.w3.org/ns/dqv#inDimension");
    pub const HAS_QUALITY_MEASUREMENT: N = n!("http://www.w3.org/ns/dqv#hasQualityMeasurement");
    pub const IS_MEASUREMENT_OF: N = n!("http://www.w3.org/ns/dqv#isMeasurementOf");
}

pub mod dcat_mqa {
    use super::N;

    pub const TRUE_SCORE: N = n!("https://data.norge.no/vocabulary/dcatno-mqa#trueScore");
    pub const SCORE: N = n!("https://data.norge.no/vocabulary/dcatno-mqa#score");
}

pub mod rdf_syntax {
    use super::N;

    pub const TYPE: N = n!("http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
}
