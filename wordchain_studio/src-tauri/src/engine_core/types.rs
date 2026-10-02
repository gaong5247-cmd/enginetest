use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticType { Win, Lose, Route }
impl StaticType {
    pub fn as_str(self) -> &'static str {
        match self { Self::Win => "win", Self::Lose => "lose", Self::Route => "route" }
    }
}

#[derive(Clone, Debug)]
pub struct WordEntry {
    pub text: String,
    pub head: char,
    pub tail: char,
    pub len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayMode { Hard, Neutral, Safe, Attack, Random }
impl PlayMode {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "neutral" => Self::Neutral,
            "safe" => Self::Safe,
            "attack" => Self::Attack,
            "random" => Self::Random,
            _ => Self::Hard,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SearchConfig {
    pub depth: u8,
    pub beam_width: usize,
    pub time_limit_ms: u64,
    pub node_limit: u64,
}
impl Default for SearchConfig {
    fn default() -> Self {
        Self { depth: 7, beam_width: 28, time_limit_ms: 2_000, node_limit: 450_000 }
    }
}

#[derive(Clone, Debug)]
pub struct CandidateAnalysis {
    pub word: String,
    pub tail: char,
    pub opponent_static: StaticType,
    pub status: &'static str,
    pub score: i32,
    pub replies: usize,
    pub opponent_attacks: usize,
    pub neutrality: u8,
    pub safety: u8,
    pub forced: bool,
}

#[derive(Clone, Debug)]
pub struct AnalysisOutput {
    pub required: Option<char>,
    pub position_static: Option<StaticType>,
    pub candidates: Vec<CandidateAnalysis>,
    pub best_word: Option<String>,
    pub pv: Vec<String>,
    pub nodes: u64,
    pub elapsed_ms: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct Engine {
    pub words: Vec<WordEntry>,
    pub(crate) by_required: HashMap<char, Vec<usize>>,
    pub(crate) index_by_text: HashMap<String, usize>,
    pub(crate) static_types: HashMap<char, StaticType>,
    pub(crate) static_depth: HashMap<char, u16>,
    pub(crate) replies_static: HashMap<char, usize>,
    pub distinct_edges: usize,
    pub node_count: usize,
}
