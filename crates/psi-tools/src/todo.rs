//! todo: a task list the model keeps for the session (psi-plan.md §10,
//! pi parity, D-34). The agent records a snapshot after each call and
//! rebuilds the list from the current branch, so it follows /tree.

use psi_ai::{Param, ParamKind, ToolSchema};
use serde_json::{Map, Value, json};

use crate::{ToolCtx, ToolOutput, int_arg, str_arg};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoList {
    pub items: Vec<Todo>,
    pub next_id: u64,
}

impl Default for TodoList {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }
}

impl TodoList {
    /// The snapshot kept in the session's `todo` entries.
    pub fn to_json(&self) -> Value {
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|t| json!({"id": t.id, "text": t.text, "done": t.done}))
            .collect();
        json!({"todos": items, "next_id": self.next_id})
    }

    pub fn from_json(v: &Value) -> Self {
        let items = v["todos"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| Todo {
                id: t["id"].as_u64().unwrap_or(0),
                text: t["text"].as_str().unwrap_or("").to_owned(),
                done: t["done"].as_bool().unwrap_or(false),
            })
            .collect();
        Self {
            items,
            next_id: v["next_id"].as_u64().unwrap_or(1),
        }
    }

    /// One line per item: `[x] #1: text`.
    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return "No todos.".into();
        }
        self.items
            .iter()
            .map(|t| format!("[{}] #{}: {}", if t.done { "x" } else { " " }, t.id, t.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        name: "todo",
        description: "Keep a task list for this session. Actions: list; add (text); toggle (id) marks an item done or not done; clear.".into(),
        params: vec![
            Param::new("action", ParamKind::String, "list, add, toggle or clear"),
            Param::new("id", ParamKind::Integer, "The item's number, for toggle").optional(),
            Param::new("text", ParamKind::String, "The item, for add").optional(),
        ],
        example: json!({"action": "add", "text": "write the tests"}),
    }
}

pub(crate) fn run(args: &Map<String, Value>, ctx: &ToolCtx) -> ToolOutput {
    let Ok(mut list) = ctx.todos.lock() else {
        return ToolOutput::err("The todo list is unavailable.");
    };
    let action = str_arg(args, "action").trim().to_ascii_lowercase();
    match action.as_str() {
        "list" | "" => ToolOutput::ok(list.render()),
        "add" => {
            let text = str_arg(args, "text").trim();
            if text.is_empty() {
                return ToolOutput::err(
                    "add needs text. Example: todo {\"action\": \"add\", \"text\": \"write the tests\"}",
                );
            }
            let id = list.next_id;
            list.next_id += 1;
            list.items.push(Todo {
                id,
                text: text.to_owned(),
                done: false,
            });
            ToolOutput::ok(format!("Added #{id}: {text}"))
        }
        // Words models use for marking an item done.
        "toggle" | "done" | "complete" | "check" => {
            let Some(id) = int_arg(args, "id") else {
                return ToolOutput::err(format!(
                    "{action} needs the item's id. Example: todo {{\"action\": \"toggle\", \"id\": 1}}\n{}",
                    list.render()
                ));
            };
            let set_done = action != "toggle";
            let Some(t) = list.items.iter_mut().find(|t| t.id as i64 == id) else {
                return ToolOutput::err(format!("There is no todo #{id}.\n{}", list.render()));
            };
            t.done = if set_done { true } else { !t.done };
            let state = if t.done { "done" } else { "not done" };
            ToolOutput::ok(format!("#{id} is {state}: {}", t.text))
        }
        "clear" => {
            let n = list.items.len();
            *list = TodoList::default();
            ToolOutput::ok(format!("Cleared {n} todos."))
        }
        other => ToolOutput::err(format!(
            "Unknown action '{other}'. Use list, add, toggle or clear."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(ctx: &ToolCtx, args: Value) -> ToolOutput {
        let Value::Object(a) = args else { panic!() };
        run(&a, ctx)
    }

    #[test]
    fn add_toggle_list_clear() {
        let d = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(d.path(), d.path(), 10_000);
        assert_eq!(call(&ctx, json!({"action": "list"})).text, "No todos.");
        assert_eq!(
            call(&ctx, json!({"action": "add", "text": "a"})).text,
            "Added #1: a"
        );
        call(&ctx, json!({"action": "add", "text": "b"}));
        assert_eq!(
            call(&ctx, json!({"action": "toggle", "id": 1})).text,
            "#1 is done: a"
        );
        // "done" always marks done.
        assert_eq!(
            call(&ctx, json!({"action": "done", "id": 1})).text,
            "#1 is done: a"
        );
        assert_eq!(
            call(&ctx, json!({"action": "list"})).text,
            "[x] #1: a\n[ ] #2: b"
        );
        let e = call(&ctx, json!({"action": "toggle", "id": 9}));
        assert!(e.is_error && e.text.starts_with("There is no todo #9.\n[x] #1: a"));
        assert!(call(&ctx, json!({"action": "add"})).is_error);
        assert!(call(&ctx, json!({"action": "remove"})).is_error);
        let snap = ctx.todos.lock().unwrap().to_json();
        assert_eq!(TodoList::from_json(&snap), *ctx.todos.lock().unwrap());
        assert_eq!(
            call(&ctx, json!({"action": "clear"})).text,
            "Cleared 2 todos."
        );
        assert_eq!(ctx.todos.lock().unwrap().next_id, 1);
    }
}
