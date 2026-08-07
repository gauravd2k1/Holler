# Holler .claude/ configuration — install instructions

Copy these into `C:\Code\Holler\.claude\` so the tree looks like:

```
C:\Code\Holler\.claude\
├── settings.json          (replace your existing one)
├── agents\
│   ├── go-builder.md
│   ├── pos-builder.md
│   ├── rust-edge-builder.md
│   └── verifier.md
└── commands\
    └── milestone.md
```

Then:

1. Add to .gitignore if not present: `.claude/worktrees/`
2. Commit the .claude/ directory (agents + commands + settings are project config, worth versioning).
3. EXIT any running Claude Code session and start fresh (`claude` in C:\Code\Holler) so settings.json and agents load.
4. Verify: type `/agents` — the four agents should be listed. Type `/permissions` — allow/deny rules should appear under project settings.
5. Kick off: `/milestone M1`

Notes:
- The deny list blocks curl/wget (agents shouldn't fetch arbitrary URLs unattended), rm -rf, and reading .env/secrets. Adjust if a legitimate need appears.
- If frontmatter fields like `isolation: worktree` are flagged as unknown by your Claude Code version, remove that line and instead tell the orchestrator to "use worktrees for builder agents" — check current syntax at https://code.claude.com/docs/en/agents
- Model note: builders pin `model: sonnet`. Run the ORCHESTRATOR session on a stronger model via /model.
