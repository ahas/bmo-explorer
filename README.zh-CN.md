[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **简体中文**

# explorer

`bmo-explorer`:作为上游的网页读取服务。它包含执行七种操作的客户端:搜索引擎检索、把页面读成文字、链接预览、页面标记、
某个地址上的图片、各引擎的状态,以及每条结果都带有许可字段的图片搜索。此外还包括编码工具以 `bmo-explorer mcp` 启动的
MCP 服务器及其 `bmo-explorer` 技能、由工具宿主放置的网页工具、保存一次调查读取内容的共享网页助手,以及站点工具所依托的
站点契约。它不提供自己的 HTTP 服务,唯一调用的服务是研究记录为某个研究分支所指定的研究单元。

## 目录结构

| 目录 | 内容 |
|---|---|
| `docs/` | 权威文档:其他所有单元都要遵循的边界(`api.md`)、网页读取(`web-access.md`)、站点契约(`site-contract.md`)、存储(`store.md`)、仓库中不得出现的个人数据(`PERSONAL-DATA.md`),以及契约本身 `openapi.json` |
| `plugin/` | 编码工具插件的技能,`skills/bmo-explorer/` |
| `src/` | 仓库唯一的 crate `bmo-explorer`,拆分为以下模块:客户端、MCP 服务器与契约文档、网页读取的各种形式、网页层、站点契约、工具宿主的调用协议、网页助手的读取以及存储 |
| `tests/` | 全部测试,其中包括跨整个仓库的检查;`tests/site-placed/` 下是针对已安装工具宿主检验已放置站点工具的测试 |
| `tools/` | 工具宿主放置的可执行文件及各自的清单和模式:`web_search`、`web_fetch`、`web_captcha`、`research`、`explorer`,以及位于 `tools/_shared/web` 的共享网页助手 |

## 构建与测试

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
(cd tests/site-placed && bun install && BMO_LIVE_TOOL_HOST=<the installed mcp-host> bun test)
```

只有当 `BMO_LIVE_TOOL_HOST` 指向已安装的 `mcp-host` 且旁边有 `bmo-pocket` 时,已放置站点测试才会运行。它会构建网页助手、
`web_search` 和 `web_fetch`,连同它自带的两个站点工具一起放置,然后检验宿主对它们的占用与调用。

## 版本

工作区的版本(semver)即仓库的版本,记录在 `VERSION` 和 Cargo 清单中。版本只在所有者宣布发布时才会变动,提交或部署都不会
改变它(docs/api.md,「Versioning」)。
