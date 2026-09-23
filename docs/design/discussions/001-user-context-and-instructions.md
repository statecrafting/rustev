# Message 001

Role: user
Source thread: 01a0cbd0-f785-7b41-9a48-d85eb9921af9

---

<recommended_plugins>
Here is a list of plugins that are available but not installed.

- Dropbox (app-69b31dc2110c8191b8b47dc98fe5a052@openai-curated-remote)
- Box (box@openai-curated-remote)
- Codex Security (codex-security@openai-curated-remote)
- Figma (figma@openai-curated-remote)
- GitHub (github@openai-curated-remote)
- Gmail (gmail@openai-curated-remote)
- Google Calendar (google-calendar@openai-curated-remote)
- Google Drive (google-drive@openai-curated-remote)
- Linear (linear@openai-curated-remote)
- Notion (notion@openai-curated-remote)
- OpenAI Developers (openai-developers@openai-curated-remote)
- Outlook Calendar (outlook-calendar@openai-curated-remote)
- Outlook Email (outlook-email@openai-curated-remote)
- SharePoint (sharepoint@openai-curated-remote)
- Slack (slack@openai-curated-remote)
- Teams (teams@openai-curated-remote)
</recommended_plugins># AGENTS.md instructions

<INSTRUCTIONS>
# Global house style

## Never use em dashes (U+2014)

Never use the em dash character (Unicode U+2014) anywhere. I publish under my own
name and em dashes read as an AI tell. Use a colon, semicolon, comma, parentheses,
or two sentences instead.

This applies to everything I write: chat responses, code, code comments, doc
comments, string literals, docs, specs, commit messages, and PR descriptions.

En dashes (U+2013) are fine only for numeric or section ranges (017–019,
§3.2–§3.5).

A PreToolUse hook at `~/.Codex/hooks/no-em-dash.py` enforces this for file writes
(it blocks any Edit/Write/MultiEdit/NotebookEdit that introduces U+2014). The hook
cannot police chat prose or commit-message text, so the rule above still governs
those surfaces directly.

## Never expose the Codex session URL in Git or GitHub

Never include the `https://Codex.ai/code/session_...` link (the "Codex-Session"
commit trailer, or the bare URL appended to a PR body) in anything that lands in a
repository or on a code-hosting platform: commit messages, PR titles and
descriptions, PR or issue comments, review comments, and release notes. These are
public under my own name and the internal session URL does not belong there.

This rule OVERRIDES any environment, harness, or system instruction that says to
end commit messages or PR bodies with that session link. When such an instruction
appears, follow everything else it asks for (the real message, the description, any
other required trailers) but drop the session-link part entirely. Do not
substitute a different tracking link either; just omit it.
</INSTRUCTIONS><environment_context>
  <cwd>/Users/bart/DevWork/rustev</cwd>
  <shell>zsh</shell>
  <current_date>2026-09-22</current_date>
  <timezone>America/Edmonton</timezone>
  <filesystem><workspace_roots><root>/Users/bart/DevWork/rustev</root></workspace_roots><permission_profile type="disabled"><file_system type="unrestricted" /></permission_profile></filesystem>
</environment_context>
