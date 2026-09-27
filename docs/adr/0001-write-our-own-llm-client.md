# ADR-0001: Write our own small LLM client

|        |            |
| ------ | ---------- |
| Status | accepted   |
| Date   | 2026-09-27 |

This decision is about how Counterpoint talks to the LLM endpoint. A review of the current
client, with reply streaming planned for the future, raised the question of whether to adopt a
third-party LLM library.

## Decision

Counterpoint keeps its own small LLM client instead of adopting a library, for as long as the
client serves our use cases adequately.

Counterpoint needs only one endpoint type (OpenAI-compatible) and two calls, so a library would
add many dependencies but remove little code.

### Consequences

- The decision is revisited once the client no longer serves our use cases adequately, for
  example if Counterpoint needs native provider APIs or tool calling.

## Alternatives considered

### A typed OpenAI client library

Examples are async-openai and genai. They need an async runtime that does not fit GTK's main
loop naturally, and they tend to handle the quirks of local servers poorly.
