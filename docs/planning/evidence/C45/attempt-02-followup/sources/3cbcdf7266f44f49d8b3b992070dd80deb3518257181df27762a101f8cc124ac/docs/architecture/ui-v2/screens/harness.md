# Harness

Source: `R2App` (`CONVOS` blocks/mcp/keys at lines 760–790, `decs`, `ended`,
`archiveActive`, `sendDraft`, `startNewConvo`, `convoHint`, `follows`),
boards R2Harness, R2Harness2, R2Harness3. Chunk: C32.

The screen is a re-skin of the existing Harness engine, sessions, decisions,
MCP and sign-in modules (C19–C22); the conversation, decision and project
authority model is unchanged. D014's Harness restrictions hold: a proposal
never authorises Apply.

## Layout

- Title row "Harness" with tenant and engine status; left rail of
  conversations (lifecycle seeds appear only on non-fresh fixture tenants;
  user conversations titled from their first message; archived ones marked
  ended); main transcript; one composer per tenant and workspace (its draft kept across navigation, tenant round trips and a conversation's retirement), with messages that reached no conversation, or whose delivery was not confirmed, kept apart for recovery and never sent by themselves (provenance P50).
- Prerequisites checklist (tenant, management client, Gateway workspace,
  company sign-in) stays visible until satisfied (D057 F05).

## Lifecycles (fixture conversations)

All three are **read-only** tool-call lifecycles in the prototype; none
writes to a tenant from the Harness.

1. **blocks** — "Why were 14 prompts blocked on claims-triage overnight?":
   read-only tool call `list_security_events` (in-page Approve once /
   Decline), result summary, recommendation, follow-up "Open the review"
   that opens the injection-override topic review in Administration
   (Runtime Security, `ChangeKind::Topics`).
2. **mcp** — "Which laptops run an MCP server that is not on the allow
   list?": in the prototype the read-only call targets an AES inventory.
   AES has no admitted service (DESIGN-V2 §6), so the implemented fixture
   reads the workspace MCP server inventory the core already exposes
   (`GatewayRead::McpServers`, C22) and its follow-up opens a Gateway
   guardrail review (`ChangeKind::GatewayGuardrails`). Recorded as
   deviation P16.
3. **keys** — "Which API keys expire before the audit, and can you rotate
   them?": read-only key inventory; the Harness refuses to rotate ("rotation
   is a write; it goes through Administration and a review") and hands off
   to the key rotation flow. The follow-up text reflects the rotation stage
   (additional key issued / old key removed).

Each has approve, decline, end and follow-up paths. Declining records the
decision and the assistant's declined text; ending seals the session
(`endConversation` → archive id) and the rail shows it ended.

Tool-call approval stays as C21/C22 built it: the core decides from the
tool's write flag whether the host's native confirmation sheet is required;
read-only calls use the in-page bar. No P09 fixture exercises a write-capable
call; the native sheet path is covered by the existing C21 tests.

## Rules

- A fresh tenant's first send creates a user conversation (I12); sending in
  an ended conversation creates a new one.
- Contextual navigation into an ended conversation shows a hint with "Open
  the saved transcript" (archived transcript detail) and "Start a new
  investigation" (new conversation prefilled with the opening question). The
  hint clears on any navigation.
- A pending decision blocks tenant switch (I1).

## Fixtures and tests

- Boards R2Harness (blocks), R2Harness2 (mcp), R2Harness3 (keys). Tests
  V2-R01, V2-T01, T13; V2-T17 approve/decline/end/follow-up per lifecycle;
  V2-T18 the keys lifecycle never dispatches a mutation from the Harness.
