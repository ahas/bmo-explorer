# The store

This repository keeps what its web reading must remember between processes in one SQLite database of its
own, `explorer/explorer.db` under the data root (docs/api.md, 「Settings and data」). No other repository
reads it; what another unit needs of it is answered through docs/api.md. The store makes its tables as it
is first opened, and drops a table of the file that is not among them. Every process of this repository —
the web helper of each record, a tool, the MCP server — opens the same file, so the pacing queues hold
across them.

## The tables

### `web_engine_cookie`

Each search engine's own cookie jar (docs/web-access.md, 「One session, one investigation」).

| Column | Type | Meaning |
|---|---|---|
| `engine` | text | The engine |
| `host` | text | The cookie's host |
| `path` | text | The cookie's path |
| `name` | text | The cookie's name |
| `value` | text | Its value |
| `secure` | integer | 1 where it is sent over https alone |
| `expires_at` | integer | When it lapses, never more than 7 days after it was set |

The key is `engine`, `host`, `path` and `name`. An engine's rows are emptied when it changes identity.

### `web_engine_health`

Each engine's state as its last request left it (docs/web-access.md, 「Engine states (four)」).

| Column | Type | Meaning |
|---|---|---|
| `engine` | text | The engine, the key |
| `state` | text | `healthy`, `degraded`, `suspended` or `unknown` |
| `reason` | text, may be null | The cause and evidence that decided the state |
| `observed_at` | integer, may be null | When it was read |
| `suspended_until` | integer, may be null | Until when it rests |
| `elapsed_ms` | integer, may be null | How long the request took |
| `result_count` | integer, may be null | How many hits it answered |

### `web_engine_recipe`

How each engine is asked, as the last block that was cleared left it (docs/web-access.md, 「A block:
diagnosis, remedy, one retry」).

| Column | Type | Meaning |
|---|---|---|
| `engine` | text | The engine, the key |
| `identity` | text | The identity it is asked as |
| `interval_ms` | integer | The least time between two of its requests |
| `route` | text | `direct`, `alternate` or `browser` |
| `warm_up` | integer | 1 where its warm-up page goes first |
| `cause` | text, may be null | The cause that set the recipe |
| `evidence` | text, may be null | What decided the cause |
| `set_at` | integer | When it was set |

### `web_host_pacing`

The pacing queues (docs/web-access.md, 「Pacing」): one row per host, and one per engine under
`engine:<name>`, its spaces written as underscores.

| Column | Type | Meaning |
|---|---|---|
| `key` | text | The host or the engine, the key |
| `next_at` | integer | The earliest moment the next request may start |
| `updated_at` | integer | When the row was last moved |

## Times

Every time in the store is in epoch milliseconds.
