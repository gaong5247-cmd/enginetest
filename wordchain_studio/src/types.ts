export type EngineStats = {
  words: number;
  nodes: number;
  distinctEdges: number;
  source: string;
  cachePath: string;
  loadedFromCache: boolean;
};

export type Candidate = {
  word: string;
  tail: string;
  opponentStatic: "win" | "lose" | "route";
  status: "finish" | "win" | "neutral" | "danger";
  score: number;
  replies: number;
  neutrality: number;
  safety: number;
  forced: boolean;
};

export type AnalysisResult = {
  required: string | null;
  positionStatic: "win" | "lose" | "route" | null;
  candidates: Candidate[];
  bestWord: string | null;
  pv: string[];
  nodes: number;
  elapsedMs: number;
  warnings: string[];
  assumption: string;
};

export type Mode = "hard" | "neutral" | "safe" | "attack" | "random";
