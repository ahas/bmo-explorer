[English](README.md) | **한국어** | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# explorer

`bmo-explorer`: 업스트림으로서의 웹 읽기입니다. 일곱 가지 작업, 즉 검색 엔진 검색, 페이지를 글로 읽기, 링크 미리보기,
페이지 마크업, 주소에 있는 그림, 엔진 상태, 그리고 결과마다 라이선스 필드를 담은 그림 검색을 수행하는 클라이언트를
담고 있습니다. 또한 하네스가 `bmo-explorer mcp`로 시작하는 MCP 서버와 `bmo-explorer` 스킬, 도구 호스트가 배치하는 웹
도구, 한 조사의 읽기 기록을 유지하는 공용 웹 헬퍼, 그리고 사이트 도구가 기반으로 삼는 사이트 계약을 담고 있습니다.
자체 HTTP는 제공하지 않으며, 호출하는 서비스는 리서치 분기를 위해 리서치 기록이 지정한 리서치 유닛 하나뿐입니다.

## 구성

| 폴더 | 내용 |
|---|---|
| `docs/` | 정본 문서: 다른 모든 유닛이 맞추는 경계(`api.md`), 웹 읽기(`web-access.md`), 사이트 계약(`site-contract.md`), 저장소(`store.md`), 저장소에 두지 않는 개인 정보(`PERSONAL-DATA.md`), 그리고 계약 자체인 `openapi.json` |
| `plugin/` | 하네스 플러그인의 스킬, `skills/bmo-explorer/` |
| `src/` | 저장소의 단일 크레이트 `bmo-explorer`. 클라이언트, MCP 서버와 계약 문서, 웹 읽기의 형식, 웹 계층, 사이트 계약, 도구 호스트 호출 프로토콜, 웹 헬퍼의 읽기, 저장소 모듈로 나뉩니다 |
| `tests/` | 모든 테스트. 저장소 전체에 걸친 검사도 포함하며, `tests/site-placed/` 아래에는 설치된 도구 호스트에 대해 배치된 사이트 도구를 확인하는 테스트가 있습니다 |
| `tools/` | 도구 호스트가 배치하는 실행 파일과 각각의 매니페스트·스키마: `web_search`, `web_fetch`, `web_captcha`, `research`, `explorer`, 그리고 `tools/_shared/web` 아래의 공용 웹 헬퍼 |

## 빌드와 테스트

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
(cd tests/site-placed && bun install && BMO_LIVE_TOOL_HOST=<the installed mcp-host> bun test)
```

배치된 사이트 테스트는 `BMO_LIVE_TOOL_HOST`가 설치된 `mcp-host`를 가리키고 그 옆에 `bmo-pocket`이 있을 때만 실행됩니다.
이 테스트는 웹 헬퍼, `web_search`, `web_fetch`를 빌드하고, 자체 사이트 도구 두 개와 함께 배치한 뒤, 호스트가 이들을
점유하고 호출하는 과정을 확인합니다.

## 버전

워크스페이스의 버전(semver)이 곧 저장소의 버전이며, `VERSION`과 Cargo 매니페스트에 기록됩니다. 버전은 소유자가 선언한
릴리스에서만 올라가며, 커밋이나 배포로는 바뀌지 않습니다(docs/api.md, 「Versioning」).
