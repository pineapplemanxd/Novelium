pub mod epub;
pub mod text_html;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedChapter {
    pub chapter_number: f64,
    pub title: String,
    pub html: String,
    pub paragraphs: Vec<String>,
    pub word_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedBook {
    pub title: String,
    pub author: String,
    pub description: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub rating: Option<f64>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub cover_bytes: Option<Vec<u8>>,
    pub cover_ext: Option<String>,
    pub chapters: Vec<ParsedChapter>,
}
