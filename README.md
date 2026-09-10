# Python Chess Engine System

이 프로젝트는 **탐색과 평가를 분리한 전통적인 체스 엔진 개발 시스템**입니다.

- Alpha-Beta / Negamax / Iterative Deepening
- Transposition Table + Zobrist hash
- TT move, MVV-LVA capture, killer, history, PVS, null move, LMR
- Quiescence search
- 64-square × 12-piece sparse feature NNUE-style evaluator
- Make/Undo와 incremental accumulator
- UCI: `uci`, `isready`, `ucinewgame`, `position`, `go depth`, `go movetime`,
  `go wtime/btime`, `stop`, `quit`
- 표준 Perft position 검증
- Self-play training data generator
- Current vs Previous model Arena 및 승격 조건
- Windows PyInstaller packaging

## Run locally

```bash
python tools/initialize_models.py
printf "uci\nisready\nposition startpos\ngo depth 4\nquit\n" | python Engine.py
python -m unittest discover -s tests -v
python tools/benchmark.py --depth 4
```

`python-chess`는 검색 또는 보드 구현에 사용하지 않습니다. 보드·비트보드
공격 테이블·합법 수 생성·make/undo는 `chess_engine/board.py`에 있습니다.

## Windows build

Windows 개발자 명령 프롬프트 또는 PowerShell에서:

```powershell
.\build_windows.ps1
```

결과:

```text
dist/
  Engine.exe
  Selfplay.exe
  Arena.exe
  models/
  games/
  data/
```

`Engine.exe`는 콘솔 UCI 엔진입니다. `Selfplay.exe`와 `Arena.exe`는 Tkinter
GUI이며 Python이 설치되지 않은 PC에서도 실행되도록 PyInstaller가 런타임을
함께 번들합니다.

## Model workflow

1. Selfplay에서 여러 CPU worker로 `data/training.jsonl`과 `games/games.pgn` 생성
2. 생성된 position에 teacher score를 추가하거나 외부 학습 스크립트로 새 `.nnue` 생성
3. Arena에서 `current.nnue`와 `previous.nnue`를 설정
4. 최소 게임 수와 통과 점수를 설정해 대국
5. 통과하면 `Promote current`로 이전 모델을 갱신

## Stockfish teacher data from public master games

매그너스 칼슨과 히카루 나카무라의 개인 API를 사용하는 것이 아니라,
Chess.com이 제공하는 공개 월별 게임 아카이브 API를 사용해 기보를 수집할 수
있습니다. Stockfish는 로컬 `stockfish` 실행 파일로 각 포지션을 평가하고,
평가값은 White 관점의 centipawn으로 `data/master_stockfish.jsonl`에 저장됩니다.

```bash
python tools/collect_master_stockfish.py \
  --months 3 \
  --max-games-per-player 50 \
  --depth 14 \
  --every 2 \
  --limit-positions 2000 \
  --overwrite \
  --train \
  --model-output models/master-stockfish.nnue
```

`--limit-positions`는 여러 선수에게 균등하게 배분됩니다. API 인덱스에 아직
생성되지 않은 최신 월이 포함될 수 있어 404 월은 자동으로 건너뜁니다.
학습기는 현재의 dependency-free NNUE 체크포인트 형식을 유지하며, 더 강한
학습을 위해서는 더 많은 포지션과 여러 epoch를 사용해야 합니다.

현재 NNUE 파일은 구조가 고정된 자체 포맷을 사용합니다. 입력 768,
hidden 256, 출력 hidden 64, scalar 1이며, Board의 accumulator는 이동 시
변경된 feature만 더하고 빼도록 구현되어 있습니다.

## Validation

테스트는 시작 포지션과 Chessprogramming Perft Results의 Kiwipete/Position
3/Position 4를 사용합니다. 또한 모든 시작 수에 대해 make/undo 후 FEN,
bitboard, Zobrist, accumulator가 완전히 복원되는지 검사합니다.