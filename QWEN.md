# QWEN.md - AI Assistant Rules

This file defines the working rules for the AI assistant when working on this codebase.

---

## Language Rules

1. **Documentation Language**: All content in this file and code comments must be written in **English**.

2. **User Communication**: All responses and communication with the user must be in **Thai language** (ภาษาไทย).

3. **Code Comments**: All comments within source code must be written in **English**.

4. **Git Commit Messages**: All Git commit messages must be written in **English**.

---

## Git Workflow Rules

5. **No Automatic Commits**: The AI assistant must **NOT** execute `git commit` or `git push` commands unless explicitly instructed by the user.

6. **Commit Preparation**: When changes are ready, the assistant should:
   - Show the user what files were changed
   - Propose a draft commit message
   - Wait for explicit user approval before committing

---

## Summary

| Aspect                                | Language              |
| ------------------------------------- | --------------------- |
| Documentation (QWEN.md, README, etc.) | English               |
| User Communication                    | Thai (ภาษาไทย)        |
| Code Comments                         | English               |
| Git Commit Messages                   | English               |
| Git Actions                           | Wait for user command |

---

## Additional Notes

- These rules take precedence over any default behavior
- If there's ambiguity, ask the user for clarification in Thai
- Always confirm before making Git commits or pushes
