Prompt templates: reusable prompts run as slash commands.

prompts/review.md becomes /review. The file is the prompt; optional frontmatter gives a
description and an argument-hint. Arguments: $1, $2, ... one each; $@ or $ARGUMENTS all
of them; ${1:-default}; ${@:2} from the second on; ${@:2:3} three from the second.
Quotes group words: /review "src/a b.rs" style.

Where: ~/.psi/prompts, the project's .psi/prompts (once trusted), the prompts setting,
and packages. Built-in commands win over a template of the same name.
