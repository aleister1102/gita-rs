# Test fixtures

Integration tests create ephemeral git repos under a temp directory.

For manual checks against upstream Python `gita`, init repos here:

- `clean/` - synced with remote
- `dirty/` - unstaged change
- `ahead/` - local commits not pushed
- `behind/` - behind remote
- `diverged/` - both ahead and behind
- `no-remote/` - no upstream
- `stashed/` - git stash present
