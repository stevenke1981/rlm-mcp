# rlm-mcp 優化路線圖

**版本基準：** v0.1.7 · Rust · 33 tools · `rmcp 2.2.0`  
**更新日期：** 2026-07-17  
**狀態說明：** 本文件取代過時的「再做 BM25/Docker/sandbox」建議；P0–P2 主線功能已完成。

---

## 1. 執行摘要

`rlm-mcp` 已是可用的獨立 RLM MCP 伺服器：外部 session、filter/map/reduce、遞迴 task、budget/trajectory、install/release、CI。  
下一階段目標從「能跑」轉為 **大規模效能、取消安全、文件可信、可維護性**。

| 優先級 | 主題 | 狀態（本輪） |
|--------|------|----------------|
| P0 | 文件/版本對齊 | **Done**（本 PR） |
| P0 | provider/REPL 子行程取消與超時 | **Done**（本 PR） |
| P0 | packaging 安全 env 範例 | **Done**（本 PR） |
| P1 | BM25 peek 預過濾 + bytes_scanned | **Done**（本 PR） |
| P1 | workflow triage 建議 | **Done**（本 PR） |
| P1 | 拆分 `RlmEngine` 上帝物件 | **Done**（本 PR） |
| P1 | BM25 索引持久化 | **Done**（本 PR） |
| P1 | 更細的 session/trajectory 鎖 | **Done**（本 PR） |
| P2 | BrowseComp / OOLONG-Pairs mini fixtures | **Done**（本 PR） |
| P2 | async HTTP provider（reqwest feature） | Planned |
| P2 | semantic embedding peek（optional feature） | Deferred — see [embedding-roadmap.md](embedding-roadmap.md) |

---

## 2. 已完成（勿重複開工）

- 33 RLM tools + `tools-list.snapshot.json` 契約
- BM25 in `rlm_peek`（`bm25=true`）
- Provider sandbox（`strict` / `warn` / `off`）+ [security.md](security.md)
- Docker + docker-compose（預設 strict sandbox）
- CI：fmt、clippy `-D warnings`、audit、Ubuntu 全測、main 上 Windows/macOS
- Lazy chunk store、trajectory、budget、`provider_cost_usd_est` 路徑
- `RLM_LOG_FORMAT=json|pretty`（stderr）
- 官方 `rmcp` stdio ServerHandler + typed tool router

---

## 3. P0 細節（本輪實作）

### 3.1 文件漂移

- README / packaging 標註 `rmcp` 實際版本（2.2.x）
- limitations：cost 估算狀態與程式對齊
- `RMCP_MIGRATION_TODO.md`：剩餘 follow-up 與已完成分離

### 3.2 取消傳播

問題：MCP `notifications/cancelled` 會讓 router 回傳取消，但 `spawn_blocking` 內的 command/REPL 子行程可能繼續跑。

解法：

1. thread-local `CancellationToken`（`src/rlm/cancel.rs`）
2. `McpServer::invoke_tool` 在 blocking worker 安裝 cancel guard
3. command provider 與 REPL 輪詢 `try_wait`，取消或超時時 `kill` 子行程
4. `Error::Cancelled` 不寫成成功 complete

驗證：unit/integration tests（慢命令 + cancel）。

### 3.3 Packaging 安全預設範例

模板 env 註解／示例：

- `RLM_PROVIDER_SANDBOX=strict`
- `RLM_PROVIDER_ALLOWED_DIRS=...`
- `RLM_ALLOW_NETWORK` 預設不開

---

## 4. P1 後續（尚未做）

### 4.1 拆分 `RlmEngine` — **Done**

```
src/rlm/engine/
  mod.rs          # struct, new, ensure_session_budget, record
  session_ops.rs  # scan/peek/chunk/session/repl/artifact
  map_ops.rs      # map plan/claim/complete + reduce
  task_ops.rs     # task create/list/result/reduce/cancel
  observe.rs      # workflow, trajectory, budget
```

Public path unchanged: `rlm_mcp::rlm::RlmEngine`.

### 4.2 BM25 索引持久化 — **Done**

- 第一次 `bm25=true` peek 建索引 → `rlm-artifacts/<session>/bm25_v1_{cs|ci}.json`
- session `revision` 變更時失效；session delete 清 memory + disk
- 記憶體 LRU 式 cap（16 sessions）+ disk 回填
- 查詢走 inverted postings；top-k 才 resolve chunk（context radius / include_content）
- 回傳 `index_hit` / `index_source` / `index_revision` / `lines_indexed` / `bytes_scanned`

### 4.3 併發鎖 — **Done**

- **Trajectory:** per-session `Mutex` + outer map `RwLock`；不同 session 的 `record`/`get` 互不阻塞；`RlmEngine` 直接 `Arc<TrajectoryStore>`（無外層 mutex）
- **Sessions:** `RwLock<SessionStore>`；`session_snapshot` 先 shared-read，miss 才 write hydrate；peek/chunk/map 在釋放 store 鎖後做 body I/O
- 測試：`tests/concurrent_access.rs`、`trajectory::concurrent_records_across_sessions`

### 4.4 workflow triage

本輪已加 `rlm_workflow(phase="triage")`：依 context 大小建議 direct / peek / full loop。

---

## 5. P2 產品與論文對齊

| 項目 | 說明 |
|------|------|
| BrowseComp-Plus mini | **Done** — `browsecomp_plus` suite |
| OOLONG-Pairs mini | **Done** — `oolong_pairs` suite |
| retrieval baseline | 明確文件：`retrieval_peek` vs 論文 CodeAct+BM25 |
| GHCR Docker push | main release 可選推 image |
| PR 上 Windows path_safety | 縮小 matrix 控分鐘數 |

---

## 6. 刻意不做

| 項目 | 原因 |
|------|------|
| 預設 Python REPL | 安全；safe transform 已夠用 |
| MCP Resources 暴露全文 | 需獨立 threat model |
| 依賴 cbm-mcp | 產品邊界：side-by-side only |
| 預設 embedding 進 binary | 體積與下載；見 embedding-roadmap |

---

## 7. 驗證清單

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Schema 變更時：

```powershell
cargo test write_tools_snapshot -- --ignored
```

---

## 8. 給 Agent 的執行順序

1. 讀本文件與 [limitations.md](limitations.md)
2. 不要重做「已完成」列
3. 下一刀優先：**async HTTP provider** 或 retrieval baseline 文件對齊
4. 任何 tool schema 變更必須同步 snapshot
5. 取消路徑必須 kill 子行程並回 `Cancelled` / `isError`
