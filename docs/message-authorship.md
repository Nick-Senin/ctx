# Message Authorship

Message roles are not authorship claims. Agent runtimes can persist project
instructions, expanded skills, tool results, approval prompts, and runtime
metadata as `role=user` messages.

ctx records message authorship as `human`, `automated`, or `unknown` from
provider structure. It does not classify prompt text. `unknown` is the required
result when a source cannot establish who created a message.

## Trusted inputs

- Codex and Claude prompt-history files are operator-input channels and may
  produce `human` messages.
- Codex session JSONL is ambiguous and remains `unknown`.
- Claude project transcripts use structured runtime markers to identify known
  automated records; other user-role records remain `unknown`.
- OpenCode parts with `synthetic=true` are `automated`; other user-role parts
  remain `unknown`.
- Custom history JSONL is not a trusted authorship channel and remains
  `unknown`, even if an exporter includes an authorship-like field.

All source events remain stored. Classification controls selection; it does not
delete or rewrite provider payloads.

## Consumer interfaces

Use `ctx_human_messages` for stable SQL consumption, or filter search with:

```bash
ctx search "query" --message-authorship human
```

The MCP `search` tool accepts the equivalent `message_authorship` argument.
The predicate is applied before pagination and forces event-level results so a
human sibling message cannot make a runtime-generated match appear human.

## Threat model and lifecycle

Only provider-native structural facts can confirm `human`. Instruction-looking
text is never negative proof, because an operator can paste it literally.
Classifier versions merge monotonically during repeat import, and newer
versions are the explicit correction mechanism. The disposable local database
may also be rebuilt from source histories without changing event identity.

Claude prompt-history sessions intentionally remain distinct from full project
transcript sessions. This preserves source identity and avoids guessing across
two provider files; human-only selection contains the confirmed prompt copy.
