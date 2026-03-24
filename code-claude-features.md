# Features: JSONL Session Tracing & Static Source Analysis

## 1 — Claude Code Session Tracing (`sessionTracer.ts`)

### Purpose

Tail Claude Code session transcripts in real time and emit typed events for each
meaningful entry. This turns an AI coding session into an observable stream that
other applications can consume via WebSocket.

### Transcript location

Claude Code writes one `.jsonl` file per session under:

```
~/.claude/projects/<encoded-path>/<session-id>.jsonl
```

The project path encoding replaces every `/` with `-` and strips the leading `-`.

Example: `/home/user/repos/my-app` → `--home-user-repos-my-app`

The most-recent `.jsonl` file (by `mtime`) in that directory is the active
session.

### Tail strategy

On `startWatching()`:
1. The current **byte size** of the file is recorded as `byteOffset`.
   This is the "tail" starting point — existing content is skipped entirely.
2. A `fs.watch()` listener is registered on the file.
3. On every `"change"` event, a `fs.createReadStream` is opened with
   `{ start: byteOffset }` and fed into `readline.createInterface`.
4. Each complete line is JSON-parsed and processed.
5. After the stream closes, `byteOffset` is advanced to the current file size.
   This prevents re-processing lines on the next change.

### JSONL entry structure

Each line is a JSON object with at minimum a `type` field and a nested `message`
object (or top-level `content`). Known noise is silently dropped:
- `isMeta: true` — Claude Code internal bookkeeping
- `type === "file-history-snapshot"` — snapshot entries
- `type === "progress"` — streaming progress markers

### Parsed entry types and emitted events

| JSONL `type` | Content condition | Emitted event |
|---|---|---|
| `"assistant"` | content block `type === "tool_use"` | `CLAUDE_TOOL_USE` |
| `"assistant"` | content block `type === "text"` | `CLAUDE_ASSISTANT_MESSAGE` |
| `"user"` | content is a plain string | `CLAUDE_USER_MESSAGE` |
| `"user"` | content block `type === "tool_result"` | `CLAUDE_TOOL_RESULT` |
| `"user"` | content block `type === "text"` | `CLAUDE_USER_MESSAGE` |

Text payloads are truncated to **200 characters** before broadcasting.

### Event shapes

```
CLAUDE_SESSION_FOUND  { sessionId, projectPath, transcriptPath }
CLAUDE_TOOL_USE       { sessionId, tool, input, timestamp, uuid }
CLAUDE_TOOL_RESULT    { sessionId, toolUseId, content, timestamp }
CLAUDE_USER_MESSAGE   { sessionId, content (≤200 chars), timestamp }
CLAUDE_ASSISTANT_MESSAGE { sessionId, summary (≤200 chars), timestamp }
```

`timestamp` is taken from the entry's own `timestamp` field when present,
otherwise the wall-clock time at parse time.

### Public API

```typescript
resolveTranscriptDir(projectPath: string): string
findLatestTranscript(transcriptDir: string): string | null
startWatching(projectPath, transcriptPath, broadcast): SessionState
stopWatching(): void
getSessionState(): SessionState | null
```

`stopWatching` closes the `fs.watch` handle and resets all state.
There is at most one active watcher at a time; calling `startWatching` a second
time implicitly stops the previous watcher.

---

## 2 — Static Source Analysis (`sourceAnalyzer.ts`)

### Purpose

Walk a project tree, extract import/export metadata from every source file
using regex (no AST), invert the dependency graph to compute `importedBy`,
call the Claude API for one-sentence file descriptions, persist the result to
`data/source-metadata.json`, and stream per-file events over WebSocket.

### File collection

Recursive directory walk starting at `projectPath`. Two filter rules:

- **Excluded directories** (never descended): `node_modules`, `dist`, `.git`,
  `build`, `coverage`
- **Included extensions**: `.ts`, `.tsx`, `.js`, `.jsx`, `.json`

### Per-file parsing (regex, no AST)

#### Imports

Three regex passes over the file content:

| Pattern | Captures |
|---|---|
| `import … from 'x'` / `import 'x'` | static imports |
| `import('x')` | dynamic imports |
| `require('x')` | CommonJS requires |

Classification:
- Source starts with `.` → **local file**. Resolved to an absolute path with
  extension probing (`.ts`, `.tsx`, `.js`, `.jsx`, then `/index.*`). Stored as
  a path **relative to project root**.
- Otherwise → **library**. Scoped packages (`@scope/pkg`) keep both segments;
  others take only the first path segment.

#### Exports

Three regex passes:

| Pattern | Captures |
|---|---|
| `export const/let/var/function/class/type/interface/enum Name` | named exports |
| `export default function/class Name` | default export (name or literal `"default"`) |
| `export { a, b as c }` | re-exports (resolved alias) |

`.json` files are not parsed for imports/exports.

#### File type heuristic

Priority order, applied to the filename and content:

1. `.json` extension → `config`
2. Filename is `types`, ends in `.types`, or ends in `.d` → `type`
3. Filename starts with `use` or contains `use[A-Z]` → `hook`
4. Contains `config`, `.env`, `vite`, `tsconfig`, `eslint`, `prettier` → `config`
5. Extension is `.tsx`/`.jsx` OR content has `React` or `export default function [A-Z]…` → `component`
6. Contains `util`, `helper`, `lib` → `utility`
7. Fallback → `other`

### `importedBy` graph inversion

After all files are parsed, a second pass iterates every file's `localFiles`
list and appends the importing file's path to the target file's `importedBy`
array. This is a simple O(N·M) reversal of the adjacency list.

### AI objective generation (Claude API)

Files are processed in **batches of 30**. For each batch, a single
`messages.create` call is made to `claude-opus-4-6` with a prompt that lists
each file's `path`, `type`, `lineCount`, and up to 5 exported symbol names.
The model is asked to return a JSON object mapping `path → one-sentence
description (≤120 chars)`.

The response is searched for the first `{…}` block and JSON-parsed. If parsing
fails, the batch falls back to empty strings for those files.

### Output and events

After all files are processed and objectives merged:
- `SOURCE_FILE_ANALYZED { file: FileMetadata }` is broadcast for each file.
- The complete `SourceMetadata` object is written to `data/source-metadata.json`
  via `writeSourceMetadata()`.
- `SOURCE_METADATA_READY { fileCount: number }` is broadcast.

### `SourceMetadata` shape

```
SourceMetadata {
  projectPath: string       // absolute, resolved
  analyzedAt:  string       // ISO 8601
  files: FileMetadata[]
}

FileMetadata {
  path:         string      // relative to projectPath
  type:         "component" | "hook" | "utility" | "config" | "type" | "other"
  imports: {
    localFiles: string[]    // relative paths to other project files
    libraries:  string[]    // npm package names
  }
  exports:      string[]    // exported symbol names
  importedBy:   string[]    // relative paths of files that import this one
  objective:    string      // AI one-sentence description
  illustration: null        // reserved (base64 PNG, not yet implemented)
  lastModified: string      // ISO 8601 mtime
  lineCount:    number
}
```

### Public API

```typescript
analyzeSourceFiles(projectPath: string, broadcast: (event: object) => void): Promise<SourceMetadata>
getSourceMetadata(dataDir: string): SourceMetadata | null
```

`analyzeSourceFiles` is async and intended to be called fire-and-forget from
the HTTP handler (the endpoint responds `{ status: "started" }` immediately).
