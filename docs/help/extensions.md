Extensions: Lua code that adds tools, commands, keys, UI and providers.

An extension is a .lua file or a directory with init.lua, returning
{ name, version, description, setup = function(psi) ... end }. They load from
~/.psi/extensions, the project's .psi/extensions (once trusted), --extension <path>,
and packages. A script tool is a directory with tool.json and an executable that reads
JSON arguments on stdin. /reload loads them again.

psi ext check <path> lints one; psi ext test <path> runs its *_test.lua files. The
author guide is docs/extensions.md and the API reference docs/api.md in the psi
repository.
