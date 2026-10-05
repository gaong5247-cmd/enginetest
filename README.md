# WordChain Studio

Roblox 한국 끝말잇기용 GUI 분석 엔진입니다.

## 핵심 기능

- 로블록스 한국 끝말잇기 사전 기반 분석
- Hard / Neutral / Safe / Attack / Random 모드
- 모든 라운드의 첫 수는 안전한 중립 수 우선
- 상대가 이을 수 있는 답변 수와 공격 가능한 응수 수 표시
- 일반 심층 분석 + 별도 0.1초 초고속 분석
- PV(예상 최선 진행)와 다음 단어 표시
- 1라운드 → 2라운드 → 3라운드 → ... 무제한 진행
- 이전 라운드에서 사용한 단어는 이후 라운드에서 재사용 금지
- 새게임만 전체 사용 기록 초기화
- Windows EXE / NSIS 설치파일 / Android APK 빌드

## 개발 실행

```bash
npm install
npm run tauri -- dev
```

## Windows 빌드

```bash
npm install
npm run tauri -- build --bundles nsis
```

## Android 빌드

Tauri 2 Android 개발 환경과 Android SDK/NDK가 필요합니다.

```bash
npm install
npm run tauri -- android init
npm run tauri -- android build --debug --apk --target aarch64 --target armv7
```

GitHub Actions의 **WordChain Studio - EXE and APK** 워크플로에서도 Windows와 Android를 자동 빌드합니다.

## 분석 동작

라운드 시작 시 제시어 한 글자를 입력합니다.

예:

```text
1라운드 시작!
제시어: 술
```

라운드 첫 단어는 현재 AI 모드와 관계없이 중립 정책을 사용합니다. 중립 정책은 상대가 충분히 많은 단어로 대응할 수 있으면서, 상대에게 즉시 강한 공격 루트를 주지 않는 후보를 우선합니다.

**다음 라운드로 가기**는 현재 라운드의 단어들을 누적 금지 목록에 추가한 뒤 라운드 번호만 증가시킵니다. **새게임**은 누적 금지 목록까지 모두 초기화합니다.
