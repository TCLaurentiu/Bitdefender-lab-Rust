use serde::{Deserialize, Serialize};

pub type GenericResultError<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize)]
pub struct SearchData {
    pub terms: Vec<String>,
    pub max_length: Option<i32>,
    pub min_score: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub total: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchMatch {
    pub file_name: String,
    pub score: f64,
}