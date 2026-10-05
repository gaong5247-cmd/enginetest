import { invoke } from "@tauri-apps/api/core";
import "./styles.css";
import type { AnalysisResult, Candidate, EngineStats, Mode } from "./types";

const app = document.querySelector<HTMLDivElement>("#app")!;

let history: string[] = [];
let previousRoundWords = new Set<string>();
let stats: EngineStats | null = null;
let result: AnalysisResult | null = null;
let busy = false;
let roundNo = 1;

app.innerHTML = `
  <div class="shell">
    <header class="topbar">
      <div>
        <div class="eyebrow">ROBLOX KOREAN WORD-CHAIN ENGINE</div>
        <h1>WordChain Studio</h1>
        <p class="subtitle">라운드 누적 금지 + worst-case 최적 루트 분석</p>
      </div>
      <div class="engine-pill" id="engineStatus"><span class="dot"></span> 엔진 준비 중</div>
    </header>

    <section class="notice" id="notice">
      사전을 불러오는 중입니다. Android 분석은 CPU 작업으로 분리되어 UI가 멈추지 않게 동작합니다.
    </section>

    <section class="controls card">
      <div class="control-group grow">
        <label for="mode">AI 모드</label>
        <select id="mode">
          <option value="hard" selected>Hard · 최악 응수 기준 최선</option>
          <option value="neutral">Neutral · 강제패배를 피하는 중립</option>
          <option value="safe">Safe · 생존 우선</option>
          <option value="attack">Attack · 상대 선택지 압박</option>
          <option value="random">Random · 안전 후보 섞기</option>
        </select>
      </div>
      <div class="control-group compact">
        <label for="depth">Depth <span id="depthValue">7</span></label>
        <input id="depth" type="range" min="2" max="10" value="7" />
      </div>
      <div class="control-group compact">
        <label for="beam">Beam <span id="beamValue">28</span></label>
        <input id="beam" type="range" min="8" max="64" step="4" value="28" />
      </div>
      <button class="ghost" id="refreshDb">DB 새로고침</button>
    </section>

    <main class="grid">
      <section class="card history-panel">
        <div class="section-head">
          <div>
            <span class="kicker">GAME STATE</span>
            <h2><span id="roundLabel">1라운드</span> 진행</h2>
          </div>
          <span class="turn-count" id="turnCount">0수</span>
        </div>

        <div class="assumption-box">
          <strong>첫 제시어</strong>
          <div class="input-row">
            <input id="promptInput" maxlength="1" autocomplete="off" spellcheck="false" placeholder="예: 술" />
            <button class="primary" id="startRound">1라운드 시작!</button>
          </div>
          <p>매 라운드 첫 수는 선택한 모드와 관계없이 상대가 많이 이을 수 있으면서 공격 루트를 주지 않는 중립 수를 우선합니다.</p>
        </div>

        <div class="assumption-box">
          <strong>진행중 제시어</strong>
          <p id="ongoingPrompt">아직 게임이 시작되지 않음</p>
        </div>

        <div class="assumption-box">
          <strong>누적 사용 단어</strong>
          <p id="usedInfo">이전 라운드 0개 · 현재 라운드 0개</p>
        </div>

        <div class="chain" id="chain">
          <div class="empty">아직 단어가 없음</div>
        </div>

        <div class="input-row">
          <input id="wordInput" autocomplete="off" spellcheck="false" placeholder="상대/내가 실제로 낸 단어 입력" />
          <button id="addWord">추가 + 분석</button>
        </div>

        <div class="button-row">
          <button class="secondary" id="analyze">현재 상태 분석</button>
          <button class="primary" id="fastAnalyze">⚡ 0.1초 분석</button>
        </div>
        <div class="button-row">
          <button class="secondary" id="nextRound">다음 라운드로 가기</button>
          <button class="ghost" id="newGame">새게임</button>
        </div>
        <div class="button-row small-buttons">
          <button class="ghost" id="undo">한 수 취소</button>
          <button class="ghost danger" id="clear">현재 라운드 기록 지우기</button>
        </div>

        <div class="assumption-box">
          <strong>분석 가정</strong>
          <p id="assumption">상대는 DB에 있는 모든 합법 수를 알고 있다고 가정함.</p>
        </div>
      </section>

      <section class="card analysis-panel">
        <div class="section-head">
          <div>
            <span class="kicker">ANALYSIS</span>
            <h2>이어질 단어</h2>
          </div>
          <div class="required" id="required">필요 음절 —</div>
        </div>

        <div class="summary" id="summary">
          <div class="metric"><span>상태</span><strong>—</strong></div>
          <div class="metric"><span>탐색 노드</span><strong>—</strong></div>
          <div class="metric"><span>시간</span><strong>—</strong></div>
          <div class="metric"><span>Best</span><strong>—</strong></div>
          <div class="metric"><span>다음 단어</span><strong>—</strong></div>
        </div>

        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>#</th><th>단어</th><th>판정</th><th>Eval</th><th>상대 답변</th><th>상대 공격</th><th>중립</th><th>안전</th>
              </tr>
            </thead>
            <tbody id="candidateBody">
              <tr><td colspan="8" class="empty-row">분석 결과가 여기에 표시됨</td></tr>
            </tbody>
          </table>
        </div>
      </section>

      <aside class="card side-panel">
        <span class="kicker">PRINCIPAL VARIATION</span>
        <h2>예상 최선 진행</h2>
        <div id="pv" class="pv"><div class="empty">아직 PV 없음</div></div>

        <div class="divider"></div>
        <span class="kicker">DATABASE</span>
        <div class="db-stats" id="dbStats">
          <div><span>Words</span><strong>—</strong></div>
          <div><span>Nodes</span><strong>—</strong></div>
          <div><span>Edges</span><strong>—</strong></div>
        </div>
        <p class="micro">0.1초 분석은 별도 100ms 탐색 예산을 사용하며, 일반 분석은 더 깊은 최적 루트를 계산합니다.</p>
      </aside>
    </main>
  </div>
  <div class="toast" id="toast"></div>
`;

const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
const modeEl = $("#mode") as HTMLSelectElement;
const depthEl = $("#depth") as HTMLInputElement;
const beamEl = $("#beam") as HTMLInputElement;
const inputEl = $("#wordInput") as HTMLInputElement;
const promptEl = $("#promptInput") as HTMLInputElement;

function setBusy(value: boolean, message?: string) {
  busy = value;
  document.body.classList.toggle("busy", value);
  for (const button of document.querySelectorAll<HTMLButtonElement>("button")) button.disabled = value;
  if (message) $("#engineStatus").innerHTML = `<span class="dot pulse"></span> ${message}`;
}

function toast(message: string, kind: "ok" | "error" = "ok") {
  const el = $("#toast");
  el.textContent = message;
  el.className = `toast show ${kind}`;
  window.setTimeout(() => (el.className = "toast"), 2600);
}

function statusLabel(status: Candidate["status"]) {
  return ({ finish: "FINISH", win: "WIN", neutral: "NEUTRAL", danger: "DANGER" } as const)[status];
}

function formatEval(score: number, forced: boolean) {
  if (forced && score > 0) return "+MATE";
  if (forced && score < 0) return "-MATE";
  const value = score / 100;
  return `${value >= 0 ? "+" : ""}${value.toFixed(2)}`;
}

function lastRequired(): string | null {
  const last = history.at(-1);
  if (!last) return null;
  return Array.from(last).at(-1) ?? null;
}

function renderHistory() {
  $("#roundLabel").textContent = `${roundNo}라운드`;
  $("#startRound").textContent = `${roundNo}라운드 시작!`;
  $("#turnCount").textContent = `${history.length}수`;
  $("#usedInfo").textContent = `이전 라운드 ${previousRoundWords.size.toLocaleString()}개 · 현재 라운드 ${history.length.toLocaleString()}개 · 총 ${(previousRoundWords.size + history.length).toLocaleString()}개`;

  const ongoing = lastRequired();
  $("#ongoingPrompt").textContent = ongoing
    ? `제시어: ${ongoing}`
    : promptEl.value.trim()
      ? `첫 제시어 대기: ${promptEl.value.trim()}`
      : `${roundNo}라운드 첫 제시어 입력 대기`;

  const chain = $("#chain");
  if (!history.length) {
    chain.innerHTML = `<div class="empty">${roundNo}라운드에서 아직 사용한 단어가 없음</div>`;
    return;
  }
  chain.innerHTML = history
    .map((word, i) => `<div class="word-chip"><span>${i + 1}</span>${escapeHtml(word)}</div>`)
    .join(`<div class="arrow">→</div>`);
}

function renderStats() {
  if (!stats) return;
  $("#dbStats").innerHTML = `
    <div><span>Words</span><strong>${stats.words.toLocaleString()}</strong></div>
    <div><span>Nodes</span><strong>${stats.nodes.toLocaleString()}</strong></div>
    <div><span>Edges</span><strong>${stats.distinctEdges.toLocaleString()}</strong></div>`;
  $("#engineStatus").innerHTML = `<span class="dot good"></span> ${stats.words.toLocaleString()} words · ${stats.loadedFromCache ? "CACHE" : "FRESH"}`;
}

function renderAnalysis() {
  if (!result) return;
  $("#required").textContent = result.required ? `필요 음절 ${result.required}` : "첫 수 자유";
  $("#assumption").textContent = result.assumption;
  const best = result.candidates[0];
  const nextWord = result.pv.length > 1 ? result.pv[1] : "—";

  $("#summary").innerHTML = `
    <div class="metric"><span>현재 상태</span><strong class="state-${result.positionStatic ?? "route"}">${(result.positionStatic ?? "start").toUpperCase()}</strong></div>
    <div class="metric"><span>탐색 노드</span><strong>${result.nodes.toLocaleString()}</strong></div>
    <div class="metric"><span>시간</span><strong>${result.elapsedMs} ms</strong></div>
    <div class="metric"><span>Best</span><strong>${best ? escapeHtml(best.word) : "없음"}</strong></div>
    <div class="metric"><span>다음 단어</span><strong>${escapeHtml(nextWord)}</strong></div>`;

  const body = $("#candidateBody");
  if (!result.candidates.length) {
    body.innerHTML = `<tr><td colspan="8" class="empty-row">이어갈 수 있는 단어가 없음</td></tr>`;
  } else {
    body.innerHTML = result.candidates
      .map((c, i) => `
        <tr data-word="${escapeAttr(c.word)}" class="candidate-row">
          <td>${i + 1}</td>
          <td><button class="word-button" data-word="${escapeAttr(c.word)}">${escapeHtml(c.word)}<small>→ ${escapeHtml(c.tail)}</small></button></td>
          <td><span class="badge ${c.status}">${statusLabel(c.status)}</span></td>
          <td class="eval ${c.score >= 0 ? "positive" : "negative"}">${formatEval(c.score, c.forced)}</td>
          <td>${c.replies.toLocaleString()}</td>
          <td>${c.opponentAttacks.toLocaleString()}</td>
          <td><div class="bar"><i style="width:${c.neutrality}%"></i><span>${c.neutrality}</span></div></td>
          <td><div class="bar"><i style="width:${c.safety}%"></i><span>${c.safety}</span></div></td>
        </tr>`)
      .join("");
  }

  const pv = $("#pv");
  pv.innerHTML = result.pv.length
    ? result.pv.map((w, i) => `<div class="pv-row"><span>${i + 1}</span><strong>${escapeHtml(w)}</strong></div>`).join("")
    : `<div class="empty">PV 없음</div>`;

  document.querySelectorAll<HTMLButtonElement>(".word-button").forEach((button) => {
    button.addEventListener("click", async () => {
      const word = button.dataset.word!;
      if (previousRoundWords.has(word) || history.includes(word)) {
        return toast("이미 이번 게임에서 사용한 단어임", "error");
      }
      history.push(word);
      renderHistory();
      await analyze();
    });
  });

  if (result.warnings.length) toast(result.warnings[0]);
}

async function init(forceRefresh = false) {
  setBusy(true, forceRefresh ? "DB 갱신 중" : "사전 로딩 중");
  try {
    stats = await invoke<EngineStats>("initialize_engine", { request: { forceRefresh } });
    renderStats();
    $("#notice").textContent = "엔진 준비 완료. 다음 라운드로 넘어가도 이전 라운드의 단어는 자동으로 금지됩니다.";
  } catch (error) {
    $("#engineStatus").innerHTML = `<span class="dot bad"></span> 로딩 실패`;
    toast(String(error), "error");
  } finally {
    setBusy(false);
  }
}

function openingPrompt(): string | null {
  if (history.length) return null;
  const value = promptEl.value.trim();
  return value || null;
}

async function analyze(overrideMode?: Mode, fast = false) {
  if (!stats) return toast("먼저 사전을 불러와야 함", "error");
  if (!history.length) {
    const prompt = openingPrompt();
    if (!prompt) return toast("첫 제시어를 한 글자로 입력해 주세요", "error");
    if (Array.from(prompt).length !== 1) return toast("첫 제시어는 한 글자만 입력", "error");
  }

  setBusy(true, fast ? "⚡ 0.1초 분석 중" : history.length ? "현재 제시어 분석 중" : "첫 수 중립 분석 중");
  try {
    result = await invoke<AnalysisResult>("analyze_chain", {
      request: {
        history,
        excludedWords: Array.from(previousRoundWords),
        startChar: openingPrompt(),
        fast,
        mode: overrideMode ?? (modeEl.value as Mode),
        depth: Number(depthEl.value),
        beamWidth: Number(beamEl.value),
        maxCandidates: fast ? 8 : 24,
      },
    });
    renderAnalysis();
    renderStats();
  } catch (error) {
    toast(String(error), "error");
  } finally {
    setBusy(false);
  }
}

async function startRound() {
  if (history.length) return toast("현재 라운드가 이미 진행 중임", "error");
  const prompt = promptEl.value.trim();
  if (Array.from(prompt).length !== 1) return toast("제시어를 한 글자로 입력해 주세요", "error");

  await analyze(modeEl.value as Mode);
  if (!result?.bestWord) {
    return toast(`제시어 '${prompt}'로 시작할 안전한 미사용 단어를 찾지 못함`, "error");
  }

  const first = result.bestWord;
  history.push(first);
  renderHistory();
  toast(`${roundNo}라운드 시작 · 제시어 ${prompt} → ${first}`);
  await analyze();
}

async function addTypedWord() {
  const word = inputEl.value.trim();
  if (!word) return;
  if (previousRoundWords.has(word)) return toast("이전 라운드에서 이미 사용한 단어임", "error");
  if (history.includes(word)) return toast("현재 라운드에서 이미 사용한 단어임", "error");
  history.push(word);
  inputEl.value = "";
  renderHistory();
  await analyze();
}

function resetAnalysisView() {
  result = null;
  $("#candidateBody").innerHTML = `<tr><td colspan="8" class="empty-row">분석 결과가 여기에 표시됨</td></tr>`;
  $("#pv").innerHTML = `<div class="empty">아직 PV 없음</div>`;
  $("#required").textContent = "필요 음절 —";
  $("#summary").innerHTML = `
    <div class="metric"><span>상태</span><strong>—</strong></div>
    <div class="metric"><span>탐색 노드</span><strong>—</strong></div>
    <div class="metric"><span>시간</span><strong>—</strong></div>
    <div class="metric"><span>Best</span><strong>—</strong></div>
    <div class="metric"><span>다음 단어</span><strong>—</strong></div>`;
}

function nextRound() {
  if (!history.length) return toast("현재 라운드에서 사용한 단어가 없음", "error");
  for (const word of history) previousRoundWords.add(word);
  history = [];
  roundNo += 1;
  promptEl.value = "";
  inputEl.value = "";
  resetAnalysisView();
  renderHistory();
  promptEl.focus();
  toast(`${roundNo}라운드 준비 · 이전 ${previousRoundWords.size}개 단어 재사용 금지`);
}

function newGame() {
  history = [];
  previousRoundWords.clear();
  roundNo = 1;
  promptEl.value = "";
  inputEl.value = "";
  resetAnalysisView();
  renderHistory();
  promptEl.focus();
  toast("완전 새게임 · 모든 사용단어 기록 초기화");
}

$("#startRound").addEventListener("click", () => void startRound());
promptEl.addEventListener("input", renderHistory);
promptEl.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !busy) void startRound();
});

$("#addWord").addEventListener("click", addTypedWord);
inputEl.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !busy) void addTypedWord();
});

$("#analyze").addEventListener("click", () => void analyze());
$("#fastAnalyze").addEventListener("click", () => void analyze(undefined, true));
$("#nextRound").addEventListener("click", nextRound);
$("#newGame").addEventListener("click", newGame);

$("#undo").addEventListener("click", async () => {
  history.pop();
  renderHistory();
  if (history.length) await analyze();
  else resetAnalysisView();
});

$("#clear").addEventListener("click", () => {
  history = [];
  resetAnalysisView();
  renderHistory();
});

$("#refreshDb").addEventListener("click", () => void init(true));

depthEl.addEventListener("input", () => ($("#depthValue").textContent = depthEl.value));
beamEl.addEventListener("input", () => ($("#beamValue").textContent = beamEl.value));

const htmlEscapes: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  "'": "&#39;",
  '"': "&quot;",
};
function escapeHtml(value: string) {
  return value.replace(/[&<>'"]/g, (ch) => htmlEscapes[ch]);
}
function escapeAttr(value: string) {
  return escapeHtml(value);
}

renderHistory();
void init(false);
