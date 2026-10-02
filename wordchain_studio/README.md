# WordChain Studio

로블록스 **한국 끝말잇기**용 GUI 분석기 / AI 실험 프로젝트.

## 목표

- Windows: 실행 파일(`.exe`) + NSIS 설치 파일 생성
- Android: 설치 가능한 debug APK 생성
- 현재까지 이어진 단어를 입력하면 사용된 단어를 제거하고 다음 수를 다시 분석
- 상대는 **현재 DB의 모든 합법 단어를 알고 있고, 항상 자신에게 가장 유리한 수를 둔다**고 가정하는 worst-case 탐색
- `첫단어 시작!`은 상대가 이어갈 수 있으면서 정적 `route`에 가까운 중립 단어를 우선 선택
- Hard / Neutral / Safe / Attack / Random 모드
- 로블록스식 두음법칙 반영 (`름 -> 름/늠/음` 특수 처리 포함)

## 구조

- `src/` : 반응형 분석 GUI (Vite + TypeScript)
- `src-tauri/src/engine_core.rs` : 독립 구현한 Rust 끝말잇기 탐색 엔진
- `src-tauri/src/lib.rs` : Tauri 명령, 사전 다운로드/캐시, GUI bridge
- `scripts/ensure_android_internet.py` : Android 네트워크 권한 확인
- `../.github/workflows/wordchain-studio.yml` : EXE/APK 자동 빌드

### 엔진 개요

1. 사전 단어를 `head -> tail` 방향 그래프로 구성
2. 로블록스 두음 규칙을 적용해 각 필요 음절의 합법 수를 인덱싱
3. 정적 그래프를 `win / lose / route`로 retrograde 분류
4. 실제 대국에서는 이미 사용한 단어를 제거
5. alpha-beta negamax + transposition table + move ordering + beam 제한으로 동적 재탐색
6. Hard 모드는 각 후보에 대해 상대의 최선 응수를 가정해 점수를 계산
7. Neutral 모드는 `route`, 충분한 상대 답변 수, 낮은 강제패배 위험을 우선

> 완전한 게임 트리를 끝까지 전수 탐색하는 것은 50만 단어 규모에서 현실적으로 불가능하므로, 동적 탐색은 설정한 depth/beam 안에서 계산합니다. 정적 win/lose/route 분류는 전체 음절 그래프를 대상으로 사전 계산합니다.

## 사전 데이터

앱 소스 저장소에는 로블록스 사전 전체를 복제해서 넣지 않습니다. 첫 실행 시 아래 공개 데이터 위치에서 내려받아 앱 데이터 디렉터리에 캐시합니다.

- `https://raw.githubusercontent.com/singrum/KoreanDict/main/roblox`

현재 확인한 데이터는 약 54.8만 줄 규모입니다. `DB 새로고침`으로 다시 받을 수 있습니다.

## 참고 프로젝트와 라이선스 주의

설계 조사 과정에서 `singrum/ggeugle`의 공개 구현과 로블록스 전용 규칙 구성을 참고했습니다. 다만 조사 시점에 저장소 루트에서 명시적인 LICENSE 파일을 확인하지 못했기 때문에, 이 서브프로젝트는 해당 소스 파일을 복사하지 않고 **독립적으로 Rust 엔진과 GUI를 구현**했습니다.

`KoreanDict` 데이터도 재배포 권한은 별도로 확인해야 합니다. 그래서 데이터 파일 자체는 이 저장소에 포함하지 않고 실행 시 원본 위치에서 받습니다. 공개 배포/상용 배포 전에는 데이터 소유자의 이용 조건을 확인하세요.

## 로컬 실행

### 개발 GUI

```bash
cd wordchain_studio
npm install
npm run tauri -- dev
```

### Windows EXE

```powershell
cd wordchain_studio
npm install
npm run tauri -- build --bundles nsis
```

주요 결과물:

- `src-tauri/target/release/wordchain-studio.exe`
- `src-tauri/target/release/bundle/nsis/*.exe`

### Android APK

Tauri Android 요구사항(Android SDK/NDK, Java, Rust Android targets)을 설치한 뒤:

```bash
cd wordchain_studio
npm install
npm run tauri -- android init --ci
python scripts/ensure_android_internet.py
npm run tauri -- android build --debug --apk --target aarch64 --target armv7
```

`--debug`를 쓰는 이유는 별도 배포용 키 없이도 바로 설치 가능한 테스트 APK를 만들기 위해서입니다. 정식 배포 시에는 release keystore를 설정하세요.

## 분석 방식 해석

후보 수의 `Eval`은 현재 차례 플레이어 관점입니다.

- 큰 양수: 현재 플레이어에게 유리
- 큰 음수: 현재 플레이어에게 위험
- `+MATE / -MATE`: 설정된 탐색 깊이 안에서 강제 종료가 확인됨
- `NEUTRAL`: 상대 차례의 정적 음절이 route/cycle 영역
- `WIN`: 상대에게 넘긴 음절이 정적 lose 영역
- `DANGER`: 상대에게 넘긴 음절이 정적 win 영역

후보 행을 누르면 그 단어를 실제 진행 기록에 추가하고 다음 상태를 즉시 재분석합니다.
