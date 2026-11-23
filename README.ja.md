[English](README.md) | [한국어](README.ko.md) | **日本語** | [简体中文](README.zh-CN.md)

# explorer

`bmo-explorer`:アップストリームとしての Web 読み取りです。7 つの操作、すなわち検索エンジンでの検索、ページを文章として
読むこと、リンクのプレビュー、ページのマークアップ、アドレスにある画像、エンジンの稼働状況、そしてヒットごとにライセンス
項目を持つ画像検索を行うクライアントを収めています。さらに、ハーネスが `bmo-explorer mcp` として起動する MCP サーバーと
`bmo-explorer` スキル、ツールホストが配置する Web ツール、1 つの調査の読み取りを保持する共有 Web ヘルパー、そしてサイト
ツールの土台となるサイト契約も含みます。独自の HTTP は提供せず、呼び出すサービスはリサーチの分岐のためにリサーチ記録が
指定するリサーチユニットだけです。

## 構成

| フォルダ | 内容 |
|---|---|
| `docs/` | 正典となるドキュメント:他のすべてのユニットが合わせる境界(`api.md`)、Web 読み取り(`web-access.md`)、サイト契約(`site-contract.md`)、ストア(`store.md`)、リポジトリに入れない個人情報(`PERSONAL-DATA.md`)、そして契約そのものである `openapi.json` |
| `plugin/` | ハーネスプラグインのスキル、`skills/bmo-explorer/` |
| `src/` | リポジトリ唯一のクレート `bmo-explorer`。クライアント、MCP サーバーと契約ドキュメント、Web 読み取りの形式、Web 層、サイト契約、ツールホストの呼び出しプロトコル、Web ヘルパーの読み取り、ストアの各モジュールに分かれています |
| `tests/` | すべてのテスト。リポジトリ全体にまたがるチェックも含み、`tests/site-placed/` には、インストール済みのツールホストに対して配置済みサイトツールを確かめるテストがあります |
| `tools/` | ツールホストが配置する実行ファイルと、それぞれのマニフェストとスキーマ:`web_search`、`web_fetch`、`web_captcha`、`research`、`explorer`、そして `tools/_shared/web` 配下の共有 Web ヘルパー |

## ビルドとテスト

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
(cd tests/site-placed && bun install && BMO_LIVE_TOOL_HOST=<the installed mcp-host> bun test)
```

配置済みサイトのテストは、`BMO_LIVE_TOOL_HOST` がインストール済みの `mcp-host` を指し、その隣に `bmo-pocket` がある場合に
のみ実行されます。Web ヘルパー、`web_search`、`web_fetch` をビルドし、独自のサイトツール 2 つと一緒に配置したうえで、
ホストによるそれらの確保と呼び出しを確かめます。

## バージョン

ワークスペースのバージョン(semver)がリポジトリのバージョンであり、`VERSION` と Cargo マニフェストに記録されます。
バージョンはオーナーが宣言したリリースでのみ上がり、コミットやデプロイでは変わりません(docs/api.md、「Versioning」)。
