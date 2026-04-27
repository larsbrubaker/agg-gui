---
name: acp
description: "Add, commit, and push all pending changes, then monitor the deployment. Use when the user wants to commit and push their work, deploy changes, or says things like 'acp', 'ship it', 'push this', 'commit and push', or 'deploy'."
---

# Add, Commit, and Push

Automates the full add-commit-push-deploy workflow. This goes beyond just committing — it pushes to origin and monitors the deployment pipeline.

## Step 1: Clean Up

Before staging anything, clean up artifacts from the current session:

- Delete any temporary scripts, scratch files, or generated artifacts created during this session
- Add any files that should be kept but never checked in to `.gitignore`

## Step 2: Review and Stage

Run `git status` and `git diff` to understand all changes.

- Never stage sensitive files (`.env`, credentials, API keys, secrets)
- Stage files by name — do not use `git add -A` or `git add .`
- If something looks wrong or unexpected, ask the user before proceeding

## Step 3: Run Tests

Run tests before committing to catch issues early:

```bash
pytest tests/ --ignore=tests/selenium/ -x -q --tb=short
```

If tests fail, launch the `fix-test-failures` skill/agent to diagnose and fix. Do not proceed until tests pass.

## Step 4: Commit

Write a concise commit message that explains the *why*, not just the *what*:

- Subject line: imperative mood, max 50 chars, no period
- Body (if needed): blank line after subject, wrap at 72 chars
- End with the co-author line

```bash
git commit -m "$(cat <<'EOF'
Subject line here

Optional body explaining why.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

If the pre-commit hook fails:
1. Fix the underlying code (not the tests) using the fix-test-failures skill
2. Re-stage the fixes
3. Create a NEW commit — do not amend
4. Repeat until the commit succeeds

## Step 5: Push

All work is done on `main`. Push directly:

```bash
git push origin main
```

After pushing, verify `git status` shows a clean working tree.

## Step 6: Monitor Deployment

After pushing, watch the CI/CD pipeline:

1. Find the triggered workflow run:
   ```bash
   gh run list --branch main --limit 5
   ```

2. Watch the run:
   ```bash
   gh run watch <run-id>
   ```

3. If the deployment fails, investigate:
   ```bash
   gh run view <run-id> --log-failed
   ```
   Then fix the issue and start the workflow over from Step 2.

## Important Notes

- Do NOT use `--no-verify` to bypass hooks
- Do NOT weaken tests to make them pass — fix the actual bugs
- Do NOT amend commits that have already been pushed
- If anything looks risky or unexpected, ask the user before proceeding
