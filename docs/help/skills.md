Skills: instructions for specific tasks, loaded when needed.

A skill is a SKILL.md file (in its own directory) or a .md file, with frontmatter:

---
name: pdf-tools
description: Extract text and tables from PDF files.
---
(the instructions)

Only the name and description go in the system prompt; the model reads the file when a
task matches. /skill:<name> [text] puts the whole skill into the conversation yourself.
disable-model-invocation: true keeps a skill out of the prompt (the command still works).

Where: ~/.psi/skills, ~/.agents/skills, the project's .psi/skills and .agents/skills
(once trusted), the skills setting, and packages. The first skill of a name wins.
