mod engine_core;

use engine_core::{AnalysisOutput, Engine, PlayMode, SearchConfig, StaticType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
    start_char: Option<String>,
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
    opponent_attacks: usize,
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
        .user_agent("WordChainStudio/0.2")
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
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

fn parse_start_char(value: Option<&str>) -> Result<Option<char>, String> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let mut chars = value.chars();
    let first = chars.next().unwrap();
    if chars.next().is_some() {
        return Err("제시어는 한 글자만 입력해 주세요.".to_string());
    }
    Ok(Some(first))
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

    // 50만+ 단어의 그래프 생성은 CPU 작업이므로 async/UI 런타임에서 분리한다.
    let engine = tauri::async_runtime::spawn_blocking(move || Engine::from_text(&text))
        .await
        .map_err(|e| format!("엔진 초기화 작업 실패: {e}"))??;
    let engine = Arc::new(engine);

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
async fn analyze_chain(
    state: State<'_, AppState>,
    request: AnalysisRequest,
) -> Result<AnalysisDto, String> {
    let engine = state
        .engine
        .lock()
        .map_err(|_| "엔진 잠금 오류".to_string())?
        .clone()
        .ok_or_else(|| "먼저 사전을 불러와 주세요.".to_string())?;

    let initial_required = parse_start_char(request.start_char.as_deref())?;
    if request.history.is_empty() && initial_required.is_none() {
        return Err("새 게임의 첫 제시어를 한 글자로 입력해 주세요.".to_string());
    }

    // 라운드 첫 수는 어떤 모드를 골라도 무조건 중립 정책.
    let selected_mode = PlayMode::parse(request.mode.as_deref().unwrap_or("hard"));
    let effective_mode = if request.history.is_empty() {
        PlayMode::Neutral
    } else {
        selected_mode
    };

    // 모바일은 짧은 예산으로 UI 먹통을 막고, 데스크톱은 더 깊게 허용.
    let mobile = cfg!(mobile);
    let depth_cap = if mobile { 6 } else { 10 };
    let beam_cap = if mobile { 24 } else { 48 };
    let config = SearchConfig {
        depth: request.depth.unwrap_or(if mobile { 5 } else { 7 }).clamp(1, depth_cap),
        beam_width: request
            .beam_width
            .unwrap_or(if mobile { 16 } else { 28 })
            .clamp(4, beam_cap),
        time_limit_ms: if mobile { 1_200 } else { 2_800 },
        node_limit: if mobile { 140_000 } else { 650_000 },
    };
    let history = request.history;
    let max_candidates = request.max_candidates.unwrap_or(20).clamp(1, 40);

    let output = tauri::async_runtime::spawn_blocking(move || {
        engine.analyze_with_required(
            &history,
            initial_required,
            effective_mode,
            config,
            max_candidates,
        )
    })
    .await
    .map_err(|e| format!("분석 작업 실패: {e}"))??;

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
                opponent_attacks: c.opponent_attacks,
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
        assumption: "첫 수는 모드와 무관하게 중립 수를 선택합니다. 이후에는 상대가 DB의 모든 합법 단어를 알고 자신에게 가장 유리한 응수를 고른다고 가정합니다.".to_string(),
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
