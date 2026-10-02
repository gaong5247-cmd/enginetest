mod engine_core;

use engine_core::{AnalysisOutput, Engine, PlayMode, SearchConfig, StaticType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

const ROBLOX_DICT_URL: &str = "https://raw.githubusercontent.com/singrum/KoreanDict/main/roblox";

#[derive(Default)]
struct AppState {
    engine: Mutex<Option<Arc<Engine>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineStats {
    words: usize,
    nodes: usize,
    distinct_edges: usize,
    source: String,
    cache_path: String,
    loaded_from_cache: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitRequest {
    force_refresh: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisRequest {
    history: Vec<String>,
    mode: Option<String>,
    depth: Option<u8>,
    beam_width: Option<usize>,
    max_candidates: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidateDto {
    word: String,
    tail: String,
    opponent_static: String,
    status: String,
    score: i32,
    replies: usize,
    neutrality: u8,
    safety: u8,
    forced: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisDto {
    required: Option<String>,
    position_static: Option<String>,
    candidates: Vec<CandidateDto>,
    best_word: Option<String>,
    pv: Vec<String>,
    nodes: u64,
    elapsed_ms: u64,
    warnings: Vec<String>,
    assumption: String,
}

fn cache_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("roblox_words.txt"))
}

async fn download_dictionary() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("WordChainStudio/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(ROBLOX_DICT_URL)
        .send()
        .await
        .map_err(|e| format!("사전 다운로드 실패: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("사전 서버 응답 오류: {}", response.status()));
    }
    let text = response
        .text()
        .await
        .map_err(|e| format!("사전 읽기 실패: {e}"))?;
    if text.lines().count() < 100_000 {
        return Err("다운로드한 사전이 비정상적으로 작습니다.".to_string());
    }
    Ok(text)
}

#[tauri::command]
async fn initialize_engine(
    app: AppHandle,
    state: State<'_, AppState>,
    request: InitRequest,
) -> Result<EngineStats, String> {
    let path = cache_file(&app)?;
    let force = request.force_refresh.unwrap_or(false);
    let mut loaded_from_cache = path.exists() && !force;

    let text = if loaded_from_cache {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        match download_dictionary().await {
            Ok(text) => {
                std::fs::write(&path, &text).map_err(|e| e.to_string())?;
                text
            }
            Err(download_error) if path.exists() => {
                loaded_from_cache = true;
                eprintln!("{download_error}; cached dictionary is used instead");
                std::fs::read_to_string(&path).map_err(|e| e.to_string())?
            }
            Err(download_error) => return Err(download_error),
        }
    };

    let engine = Arc::new(Engine::from_text(&text)?);
    let stats = EngineStats {
        words: engine.words.len(),
        nodes: engine.node_count,
        distinct_edges: engine.distinct_edges,
        source: ROBLOX_DICT_URL.to_string(),
        cache_path: path.to_string_lossy().to_string(),
        loaded_from_cache,
    };

    *state.engine.lock().map_err(|_| "엔진 잠금 오류".to_string())? = Some(engine);
    Ok(stats)
}

#[tauri::command]
fn analyze_chain(
    state: State<'_, AppState>,
    request: AnalysisRequest,
) -> Result<AnalysisDto, String> {
    let engine = state
        .engine
        .lock()
        .map_err(|_| "엔진 잠금 오류".to_string())?
        .clone()
        .ok_or_else(|| "먼저 사전을 불러와 주세요.".to_string())?;

    let config = SearchConfig {
        depth: request.depth.unwrap_or(7).clamp(1, 12),
        beam_width: request.beam_width.unwrap_or(28).clamp(4, 96),
    };
    let output = engine.analyze(
        &request.history,
        PlayMode::parse(request.mode.as_deref().unwrap_or("hard")),
        config,
        request.max_candidates.unwrap_or(20).clamp(1, 50),
    )?;
    Ok(to_dto(output))
}

fn static_name(t: StaticType) -> String {
    t.as_str().to_string()
}

fn to_dto(output: AnalysisOutput) -> AnalysisDto {
    AnalysisDto {
        required: output.required.map(|c| c.to_string()),
        position_static: output.position_static.map(static_name),
        candidates: output
            .candidates
            .into_iter()
            .map(|c| CandidateDto {
                word: c.word,
                tail: c.tail.to_string(),
                opponent_static: static_name(c.opponent_static),
                status: c.status.to_string(),
                score: c.score,
                replies: c.replies,
                neutrality: c.neutrality,
                safety: c.safety,
                forced: c.forced,
            })
            .collect(),
        best_word: output.best_word,
        pv: output.pv,
        nodes: output.nodes,
        elapsed_ms: output.elapsed_ms,
        warnings: output.warnings,
        assumption: "상대는 현재 DB의 모든 합법 단어를 알고 있으며 매 턴 자신에게 가장 유리한 응수를 선택한다고 가정합니다.".to_string(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![initialize_engine, analyze_chain])
        .run(tauri::generate_context!())
        .expect("error while running WordChain Studio");
}
